use super::*;
use std::cell::Cell;
use std::io::{Cursor, Write};

#[test]
fn byte_limit_accepts_an_exact_fit_and_rejects_one_extra_byte() {
    let json = "{\"text\":\"界\"}";
    let parsed: serde_json::Value =
        parse_bounded(json.as_bytes(), Path::new("exact.json"), json.len() as u64).unwrap();
    assert_eq!(parsed["text"], "界");
    let too_small = parse_bounded::<serde_json::Value>(
        json.as_bytes(),
        Path::new("over.json"),
        json.len() as u64 - 1,
    );
    assert!(matches!(too_small, Err(StreamError::Transient { .. })));
    let invalid =
        parse_bounded::<serde_json::Value>(b"{x".as_slice(), Path::new("invalid.json"), 10);
    assert!(matches!(invalid, Err(StreamError::Parse { .. })));
}

struct CountedRead<'a> {
    bytes: &'a Cell<usize>,
    contents: Cursor<Vec<u8>>,
}

impl Read for CountedRead<'_> {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        let count = self.contents.read(output)?;
        self.bytes.set(self.bytes.get() + count);
        Ok(count)
    }
}

#[test]
fn growth_after_the_metadata_check_reads_only_one_sentinel_past_the_limit() {
    let bytes = Cell::new(0);
    let contents = format!("{{\"text\":\"{}\"}}", "x".repeat(1024 * 1024)).into_bytes();
    let reader = CountedRead {
        bytes: &bytes,
        contents: Cursor::new(contents),
    };
    let result = parse_bounded::<serde_json::Value>(reader, Path::new("growing.json"), 32);
    assert!(matches!(result, Err(StreamError::Transient { .. })));
    assert_eq!(bytes.get(), 33);
}

#[test]
fn transcript_model_probes_also_refuse_oversized_whole_files() {
    use crate::operations::streams::{model_extraction::extract_model, sweep::StreamFormat};
    for (format, contents) in [
        (
            StreamFormat::AmpThreadJson,
            r#"{"messages":[{"usage":{"model":"large-model"}}]}"#,
        ),
        (
            StreamFormat::CopilotSessionJson,
            r#"{"requests":[{"modelId":"large-model"}]}"#,
        ),
    ] {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        assert_eq!(
            extract_model(file.path(), format, None).unwrap().as_deref(),
            Some("large-model")
        );
        let limit = Config::get().max_transcript_file_bytes() as u64;
        let padding = limit + 1 - contents.len() as u64;
        std::io::copy(&mut std::io::repeat(b' ').take(padding), &mut file).unwrap();
        assert!(matches!(
            read_json_file::<serde_json::Value>(file.path()),
            Err(StreamError::Transient { .. })
        ));
        assert_eq!(extract_model(file.path(), format, None).unwrap(), None);
    }
}
