use super::LogError;
use super::render::LogRenderer;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode},
    execute, queue,
    style::{Attribute, Print, SetAttribute},
    terminal::{
        self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub(super) fn stream_to_stdout(mut renderer: LogRenderer) -> Result<(), LogError> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();

    loop {
        let rendered = renderer.render_next_batch()?;
        if rendered.is_empty() {
            break;
        }

        for commit in rendered {
            match lock.write_all(commit.as_bytes()) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => return Ok(()),
                Err(error) => return Err(LogError::Io(error)),
            }
        }
        lock.flush().map_err(LogError::Io)?;
    }

    Ok(())
}

pub(super) fn run_pager(renderer: LogRenderer) -> Result<(), LogError> {
    let mut pager = LogPager::new(renderer)?;
    let mut stdout = io::stdout();
    let _guard = TerminalGuard::enter(&mut stdout)?;
    let mut scroll = 0usize;
    let mut needs_redraw = true;
    let mut last_size: Option<(u16, u16)> = None;

    loop {
        let (width, height) = terminal::size().map_err(LogError::Io)?;
        let viewport_height = usize::from(height.saturating_sub(1)).max(1);
        if needs_redraw {
            if last_size.is_some_and(|size| size != (width, height)) {
                queue!(stdout, MoveTo(0, 0), Clear(ClearType::All)).map_err(LogError::Io)?;
            }
            pager.ensure_line_loaded(scroll.saturating_add(viewport_height))?;
            draw_pager(
                &mut stdout,
                &mut pager,
                scroll,
                width,
                height,
                viewport_height,
            )?;
            last_size = Some((width, height));
            needs_redraw = false;
        }

        if !event::poll(std::time::Duration::from_millis(250)).map_err(LogError::Io)? {
            continue;
        }

        match event::read().map_err(LogError::Io)? {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    pager.ensure_line_loaded(scroll.saturating_add(viewport_height + 1))?;
                    if scroll + 1 < pager.line_count() {
                        scroll += 1;
                    }
                    needs_redraw = true;
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    scroll = scroll.saturating_sub(1);
                    needs_redraw = true;
                }
                KeyCode::PageDown | KeyCode::Char(' ') => {
                    let next = scroll.saturating_add(viewport_height);
                    pager.ensure_line_loaded(next.saturating_add(viewport_height))?;
                    scroll = next.min(pager.max_scroll(viewport_height));
                    needs_redraw = true;
                }
                KeyCode::PageUp => {
                    scroll = scroll.saturating_sub(viewport_height);
                    needs_redraw = true;
                }
                KeyCode::Home => {
                    scroll = 0;
                    needs_redraw = true;
                }
                KeyCode::End => {
                    pager.load_all()?;
                    scroll = pager.max_scroll(viewport_height);
                    needs_redraw = true;
                }
                _ => {}
            },
            Event::Resize(_, _) => {
                needs_redraw = true;
            }
            _ => {}
        }
    }

    Ok(())
}

pub(super) fn draw_pager(
    stdout: &mut io::Stdout,
    pager: &mut LogPager,
    scroll: usize,
    width: u16,
    height: u16,
    viewport_height: usize,
) -> Result<(), LogError> {
    for row in 0..viewport_height {
        queue!(stdout, MoveTo(0, row as u16), Clear(ClearType::CurrentLine))
            .map_err(LogError::Io)?;
        if let Some(line) = pager.read_line(scroll + row)? {
            queue!(
                stdout,
                Print(truncate_for_width(
                    line.trim_end_matches('\n'),
                    width as usize
                ))
            )
            .map_err(LogError::Io)?;
        }
    }

    let status = if pager.is_eof() {
        format!(
            " git-ai log  lines {}  q quit  ↑/↓ scroll  PgUp/PgDn page ",
            pager.line_count()
        )
    } else {
        format!(
            " git-ai log  loaded {} lines  q quit  ↑/↓ scroll  PgUp/PgDn page ",
            pager.line_count()
        )
    };
    queue!(
        stdout,
        MoveTo(0, height.saturating_sub(1)),
        Clear(ClearType::CurrentLine),
        SetAttribute(Attribute::Reverse),
        Print(pad_for_width(&status, width as usize)),
        SetAttribute(Attribute::Reset)
    )
    .map_err(LogError::Io)?;
    stdout.flush().map_err(LogError::Io)?;
    Ok(())
}

pub(super) fn truncate_for_width(line: &str, width: usize) -> String {
    let mut out = String::new();
    let mut visible_width = 0usize;
    let mut index = 0usize;
    let mut saw_ansi = false;
    let bytes = line.as_bytes();

    while index < bytes.len() && visible_width < width {
        if bytes[index] == 0x1b
            && let Some(end) = ansi_escape_end(bytes, index)
        {
            out.push_str(&line[index..end]);
            index = end;
            saw_ansi = true;
            continue;
        }

        let Some(ch) = line[index..].chars().next() else {
            break;
        };
        out.push(ch);
        visible_width += 1;
        index += ch.len_utf8();
    }

    if saw_ansi {
        out.push_str("\x1b[0m");
    }

    out
}

pub(super) fn ansi_escape_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&0x1b) || bytes.get(start + 1) != Some(&b'[') {
        return None;
    }

    for (index, byte) in bytes.iter().enumerate().skip(start + 2) {
        if (0x40..=0x7e).contains(byte) {
            return Some(index + 1);
        }
    }

    None
}

pub(super) fn pad_for_width(line: &str, width: usize) -> String {
    let mut value = truncate_for_width(line, width);
    let current = value.chars().count();
    if current < width {
        value.push_str(&" ".repeat(width - current));
    }
    value
}

pub(super) struct LogPager {
    pub(super) renderer: LogRenderer,
    pub(super) spool: Spool,
}

impl LogPager {
    pub(super) fn new(renderer: LogRenderer) -> Result<Self, LogError> {
        Ok(Self {
            renderer,
            spool: Spool::new()?,
        })
    }

    pub(super) fn ensure_line_loaded(&mut self, line_index: usize) -> Result<(), LogError> {
        while self.spool.line_count() <= line_index && !self.renderer.is_eof() {
            let rendered = self.renderer.render_next_batch()?;
            if rendered.is_empty() {
                break;
            }
            for commit in rendered {
                self.spool.append(&commit)?;
            }
        }
        Ok(())
    }

    pub(super) fn load_all(&mut self) -> Result<(), LogError> {
        while !self.renderer.is_eof() {
            let rendered = self.renderer.render_next_batch()?;
            if rendered.is_empty() {
                break;
            }
            for commit in rendered {
                self.spool.append(&commit)?;
            }
        }
        Ok(())
    }

    pub(super) fn read_line(&mut self, line_index: usize) -> Result<Option<String>, LogError> {
        self.spool.read_line(line_index)
    }

    pub(super) fn line_count(&self) -> usize {
        self.spool.line_count()
    }

    pub(super) fn is_eof(&self) -> bool {
        self.renderer.is_eof()
    }

    pub(super) fn max_scroll(&self, viewport_height: usize) -> usize {
        self.line_count().saturating_sub(viewport_height)
    }
}

pub(super) struct Spool {
    pub(super) path: PathBuf,
    pub(super) file: File,
    pub(super) line_offsets: Vec<u64>,
    pub(super) write_offset: u64,
}

impl Spool {
    pub(super) fn new() -> Result<Self, LogError> {
        let path = unique_spool_path();
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(LogError::Io)?;
        Ok(Self {
            path,
            file,
            line_offsets: Vec::new(),
            write_offset: 0,
        })
    }

    pub(super) fn append(&mut self, text: &str) -> Result<(), LogError> {
        self.file
            .seek(SeekFrom::Start(self.write_offset))
            .map_err(LogError::Io)?;

        if text.is_empty() {
            return Ok(());
        }

        for chunk in text.as_bytes().split_inclusive(|byte| *byte == b'\n') {
            self.line_offsets.push(self.write_offset);
            self.file.write_all(chunk).map_err(LogError::Io)?;
            self.write_offset += chunk.len() as u64;
        }

        if !text.as_bytes().ends_with(b"\n") {
            self.file.write_all(b"\n").map_err(LogError::Io)?;
            self.write_offset += 1;
        }

        self.file.flush().map_err(LogError::Io)?;
        Ok(())
    }

    pub(super) fn read_line(&mut self, line_index: usize) -> Result<Option<String>, LogError> {
        let Some(offset) = self.line_offsets.get(line_index).copied() else {
            return Ok(None);
        };

        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(LogError::Io)?;
        let mut bytes = Vec::new();
        let mut buf = [0u8; 1];
        loop {
            let read = self.file.read(&mut buf).map_err(LogError::Io)?;
            if read == 0 {
                break;
            }
            bytes.push(buf[0]);
            if buf[0] == b'\n' {
                break;
            }
        }

        Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
    }

    pub(super) fn line_count(&self) -> usize {
        self.line_offsets.len()
    }
}

impl Drop for Spool {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) fn unique_spool_path() -> PathBuf {
    let nanos = crate::model::clock::now_nanos();
    std::env::temp_dir().join(format!("git-ai-log-{}-{}.tmp", std::process::id(), nanos))
}

pub(super) struct TerminalGuard;

impl TerminalGuard {
    pub(super) fn enter(stdout: &mut io::Stdout) -> Result<Self, LogError> {
        enable_raw_mode().map_err(LogError::Io)?;
        if let Err(error) = execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), Hide) {
            let _ = disable_raw_mode();
            return Err(LogError::Io(error));
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, Show, LeaveAlternateScreen);
    }
}
