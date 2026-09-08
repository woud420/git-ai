use super::arguments::has_flag;
use super::startup::{daemon_config_from_env_or_default_paths, daemon_is_up};
use crate::operations::daemon::daemon_log_file_path;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::thread;
use std::time::Duration;

pub(super) fn handle_tail(args: &[String]) -> Result<(), String> {
    let config = daemon_config_from_env_or_default_paths()?;
    if !daemon_is_up(&config) {
        return Err("background service is not running".to_string());
    }

    let log_path =
        daemon_log_file_path(&config).map_err(|e| format!("cannot locate log: {}", e))?;
    if !log_path.exists() {
        return Err(format!("log file not found: {}", log_path.display()));
    }

    let full = has_flag(args, "--full");
    let follow = has_flag(args, "--follow") || has_flag(args, "-f");
    let lines: usize = parse_number_arg(args, "-n")
        .or_else(|| parse_number_arg(args, "--lines"))
        .unwrap_or(20);

    let file = std::fs::File::open(&log_path)
        .map_err(|e| format!("cannot open {}: {}", log_path.display(), e))?;

    if full {
        // Print entire file then continue tailing.
        let reader = BufReader::new(&file);
        for line in reader.lines() {
            let line = line.map_err(|e| e.to_string())?;
            println!("{}", line);
        }
    } else {
        // Print last N lines.
        print_last_n_lines(&file, lines).map_err(|e| e.to_string())?;
    }

    if follow {
        tail_file(file).map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

pub(super) fn parse_number_arg(args: &[String], flag: &str) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag && i + 1 < args.len() {
            return args[i + 1].parse().ok();
        }
        i += 1;
    }
    None
}

pub(super) fn print_last_n_lines(file: &std::fs::File, n: usize) -> Result<(), std::io::Error> {
    use std::io::Read;
    let metadata = file.metadata()?;
    let file_size = metadata.len();
    if file_size == 0 {
        return Ok(());
    }

    // Read up to 64KB from the end to find the last N lines.
    let read_size = file_size.min(64 * 1024) as usize;
    let mut buf = vec![0u8; read_size];
    let mut f = file;
    f.seek(SeekFrom::End(-(read_size as i64)))?;
    f.read_exact(&mut buf)?;

    let text = String::from_utf8_lossy(&buf);
    let all_lines: Vec<&str> = text.lines().collect();
    let start = all_lines.len().saturating_sub(n);
    for line in &all_lines[start..] {
        println!("{}", line);
    }

    // Seek to end so tail_file can pick up from here.
    f.seek(SeekFrom::End(0))?;
    Ok(())
}

pub(super) fn tail_file(file: std::fs::File) -> Result<(), std::io::Error> {
    let mut reader = BufReader::new(file);
    // Seek to end in case print_last_n_lines didn't (full mode).
    reader.seek(SeekFrom::End(0))?;
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n > 0 {
            print!("{}", line);
        } else {
            thread::sleep(Duration::from_millis(200));
        }
    }
}
