use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::extract_json_object;
use git_ai::operations::authorship::stats::CommitStats;
use std::fs;

fn gbk_hello_world() -> Vec<u8> {
    // "你好世界" in GBK encoding (4 characters, 8 bytes)
    // 你=0xC4E3, 好=0xBAC3, 世=0xCAC0, 界=0xBDE7
    vec![0xC4, 0xE3, 0xBA, 0xC3, 0xCA, 0xC0, 0xBD, 0xE7, b'\n']
}

fn gbk_multiline() -> Vec<u8> {
    // Three lines of GBK text
    let mut bytes = Vec::new();
    // Line 1: 你好 (hello) = C4E3 BAC3
    bytes.extend_from_slice(&[0xC4, 0xE3, 0xBA, 0xC3]);
    bytes.push(b'\n');
    // Line 2: 世界 (world) = CAC0 BDE7
    bytes.extend_from_slice(&[0xCA, 0xC0, 0xBD, 0xE7]);
    bytes.push(b'\n');
    // Line 3: 测试 (test) = B2E2 CAD4
    bytes.extend_from_slice(&[0xB2, 0xE2, 0xCA, 0xD4]);
    bytes.push(b'\n');
    bytes
}

fn latin1_bytes() -> Vec<u8> {
    // Latin-1 text with characters outside UTF-8
    // "café résumé" with Latin-1 accented chars (0xe9 = é in Latin-1, invalid as standalone UTF-8)
    b"caf\xe9 r\xe9sum\xe9\n".to_vec()
}

fn shift_jis_bytes() -> Vec<u8> {
    // Shift-JIS encoded Japanese text
    // こんにちは (konnichiwa) in Shift-JIS
    vec![
        0x82, 0xB1, 0x82, 0xF1, 0x82, 0xC9, 0x82, 0xBF, 0x82, 0xCD, b'\n',
    ]
}

fn mixed_valid_invalid_utf8() -> Vec<u8> {
    // Mix of valid UTF-8 ASCII lines and invalid UTF-8 lines
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"line one is ascii\n");
    // Invalid UTF-8 byte sequence
    bytes.extend_from_slice(&[0x80, 0x81, 0x82, b'\n']);
    bytes.extend_from_slice(b"line three is ascii\n");
    // Another invalid sequence
    bytes.extend_from_slice(&[0xFE, 0xFF, b'\n']);
    bytes.extend_from_slice(b"line five is ascii\n");
    bytes
}

mod checkpoint_attribution;
mod encoded_content;
mod stats_and_blame;
