use std::io::{BufRead, Read};

/// Result of reading a single line from a JSONL reader.
pub enum JsonlLineState {
    Eof,
    Partial,
    Complete(usize),
    /// The record exceeds the byte limit; its remainder has not been consumed.
    Oversized,
}

/// Bound raw bytes before UTF-8 decoding, including incomplete writer output.
pub fn read_jsonl_line(
    reader: &mut impl BufRead,
    line: &mut String,
    max_bytes: usize,
) -> std::io::Result<JsonlLineState> {
    let mut bytes = std::mem::take(line).into_bytes();
    bytes.clear();
    let bytes_read = reader
        .take(max_bytes.saturating_add(1) as u64)
        .read_until(b'\n', &mut bytes)?;
    if bytes_read > max_bytes {
        return Ok(JsonlLineState::Oversized);
    }
    *line = String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    if bytes_read == 0 {
        return Ok(JsonlLineState::Eof);
    }
    if !line.ends_with('\n') {
        return Ok(JsonlLineState::Partial);
    }
    Ok(JsonlLineState::Complete(bytes_read))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_read_jsonl_line_eof() {
        let data = b"";
        let mut reader = std::io::BufReader::new(&data[..]);
        let mut line = String::new();
        let result = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(result, JsonlLineState::Eof));
    }

    #[test]
    fn test_read_jsonl_line_complete() {
        let data = b"{\"id\":1}\n";
        let mut reader = std::io::BufReader::new(&data[..]);
        let mut line = String::new();
        let result = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(result, JsonlLineState::Complete(9)));
        assert_eq!(line, "{\"id\":1}\n");
    }

    #[test]
    fn test_read_jsonl_line_partial() {
        let data = b"{\"id\":1}";
        let mut reader = std::io::BufReader::new(&data[..]);
        let mut line = String::new();
        let result = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(result, JsonlLineState::Partial));
    }

    #[test]
    fn test_read_jsonl_line_multiple_lines() {
        let data = b"{\"a\":1}\n{\"b\":2}\n";
        let mut reader = std::io::BufReader::new(&data[..]);
        let mut line = String::new();

        let r1 = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(r1, JsonlLineState::Complete(8)));

        let r2 = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(r2, JsonlLineState::Complete(8)));

        let r3 = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(r3, JsonlLineState::Eof));
    }

    #[test]
    fn test_read_jsonl_line_complete_then_partial() {
        let data = b"{\"a\":1}\n{\"b\":2}";
        let mut reader = std::io::BufReader::new(&data[..]);
        let mut line = String::new();

        let r1 = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(r1, JsonlLineState::Complete(8)));

        let r2 = read_jsonl_line(&mut reader, &mut line, 1024).unwrap();
        assert!(matches!(r2, JsonlLineState::Partial));
    }
    #[test]
    fn byte_limit_split_inside_utf8_is_oversized_without_draining_the_record() {
        let mut reader = std::io::Cursor::new("abé remaining bytes\n");
        let mut line = String::new();
        assert!(matches!(
            read_jsonl_line(&mut reader, &mut line, 2).unwrap(),
            JsonlLineState::Oversized
        ));
        assert_eq!(reader.position(), 3);
        assert!(line.is_empty());
    }

    #[test]
    fn complete_line_at_exact_byte_limit_is_accepted() {
        let mut reader = std::io::Cursor::new("é\n");
        let mut line = String::new();
        assert!(matches!(
            read_jsonl_line(&mut reader, &mut line, 3).unwrap(),
            JsonlLineState::Complete(3)
        ));
        assert_eq!(line, "é\n");
    }

    #[test]
    fn invalid_utf8_inside_byte_limit_remains_an_io_error() {
        let mut reader = std::io::Cursor::new([0xff, b'\n']);
        let mut line = String::new();
        assert_eq!(
            read_jsonl_line(&mut reader, &mut line, 8)
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
    }
}
