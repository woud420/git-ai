use super::*;
use std::io::Cursor;

struct TimeoutOnce {
    bytes: Cursor<Vec<u8>>,
    after: Option<usize>,
}

impl Read for TimeoutOnce {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let length = match self.after {
            Some(after) if self.bytes.position() as usize == after => {
                self.after = None;
                return Err(io::ErrorKind::TimedOut.into());
            }
            Some(after) => output.len().min(after - self.bytes.position() as usize),
            None => output.len(),
        };
        self.bytes.read(&mut output[..length])
    }
}

#[test]
fn trace_reader_preserves_partial_utf8_across_bootstrap_timeout() {
    let mut reader = TraceReader::new(TimeoutOnce {
        bytes: Cursor::new("€\nnext\n".as_bytes().to_vec()),
        after: Some(1),
    });
    assert!(
        matches!(reader.read_line(), Err(GitAiError::IoError(e)) if e.kind() == io::ErrorKind::TimedOut)
    );
    assert_eq!(reader.read_line().unwrap().as_deref(), Some("€\n"));
    assert_eq!(reader.read_line().unwrap().as_deref(), Some("next\n"));
    assert_eq!(reader.read_line().unwrap(), None);
}

#[test]
fn resumed_frame_includes_pre_timeout_bytes_in_its_budget() {
    let mut reader = BufReader::new(TimeoutOnce {
        bytes: Cursor::new(b"1234567890\n".to_vec()),
        after: Some(4),
    });
    let mut pending = Vec::new();
    assert!(read_bounded_line(&mut reader, &mut pending, 8, "trace").is_err());
    assert_eq!(pending, b"1234");
    let error = read_bounded_line(&mut reader, &mut pending, 8, "trace").unwrap_err();
    assert_eq!(
        error.to_string(),
        "IO error: daemon trace frame exceeds 8 bytes"
    );
    assert_eq!(pending.len(), 9);
}

#[test]
fn byte_limit_rejects_a_cut_utf8_character_before_decoding() {
    let mut reader = Cursor::new("€€€\n".as_bytes());
    let error = read_json_line_bounded(&mut reader, 3).unwrap_err();
    assert_eq!(
        error.to_string(),
        "IO error: daemon control frame exceeds 3 bytes"
    );
    assert_eq!(reader.position(), 4);
}

#[test]
fn bounded_frames_preserve_eof_and_reject_invalid_utf8() {
    assert_eq!(
        read_json_line_bounded(&mut Cursor::new(b""), 8).unwrap(),
        None
    );
    assert_eq!(
        read_json_line_bounded(&mut Cursor::new(b"tail"), 8)
            .unwrap()
            .as_deref(),
        Some("tail")
    );
    let error = read_json_line_bounded(&mut Cursor::new([0xff, b'\n']), 8).unwrap_err();
    assert!(matches!(error, GitAiError::IoError(e) if e.kind() == io::ErrorKind::InvalidData));
}

#[test]
fn bounded_json_line_rejects_oversized_input_without_consuming_the_tail() {
    let mut reader = Cursor::new(b"1234567890\nnext\n".to_vec());

    let error = read_json_line_bounded(&mut reader, 8).unwrap_err();

    assert_eq!(
        error.to_string(),
        "IO error: daemon control frame exceeds 8 bytes"
    );
    assert_eq!(reader.position(), 9);
}

#[test]
fn bounded_json_line_accepts_a_frame_at_the_limit() {
    let mut reader = Cursor::new(b"1234567\nnext\n".to_vec());

    assert_eq!(
        read_json_line_bounded(&mut reader, 8).unwrap(),
        Some("1234567\n".to_string())
    );
    assert_eq!(
        read_json_line_bounded(&mut reader, 8).unwrap(),
        Some("next\n".to_string())
    );
}
