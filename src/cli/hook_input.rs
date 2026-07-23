//! Decoding for checkpoint `--hook-input` payloads: UTF-8 BOM stripping and
//! best-effort UTF-16 (LE/BE) detection for agents that pipe wide-char JSON
//! via stdin.

/// Strip a UTF-8 BOM from `input`, delegating to the byte-level helper in
/// `config`. Falls back to the original string if the stripped bytes are not
/// valid UTF-8 (should not happen for a BOM-only strip).
pub(crate) fn strip_utf8_bom(input: String) -> String {
    let stripped = crate::config::strip_utf8_bom(input.as_bytes());
    if stripped.len() == input.len() {
        return input;
    }
    String::from_utf8(stripped.to_vec()).unwrap_or(input)
}

/// Decode raw hook-input bytes to a `String`, handling a UTF-16 BOM or
/// heuristically-detected UTF-16 (LE/BE) content in addition to plain UTF-8.
pub(crate) fn decode_hook_input_bytes(bytes: Vec<u8>) -> Result<String, String> {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return decode_utf16_hook_input(&bytes[2..], Utf16Endian::Little);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return decode_utf16_hook_input(&bytes[2..], Utf16Endian::Big);
    }

    match likely_utf16_endian(&bytes) {
        Some(endian) => decode_utf16_hook_input(&bytes, endian),
        None => String::from_utf8(bytes).map_err(|e| e.to_string()),
    }
}

#[derive(Clone, Copy)]
enum Utf16Endian {
    Little,
    Big,
}

fn likely_utf16_endian(bytes: &[u8]) -> Option<Utf16Endian> {
    let sample_len = bytes.len().min(512);
    if sample_len < 8 {
        return None;
    }

    let sample = &bytes[..sample_len];
    let even_nuls = sample.iter().step_by(2).filter(|&&b| b == 0).count();
    let odd_nuls = sample
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|&&b| b == 0)
        .count();
    let min_nuls = sample_len / 8;

    if odd_nuls > min_nuls && odd_nuls > even_nuls.saturating_mul(4) {
        Some(Utf16Endian::Little)
    } else if even_nuls > min_nuls && even_nuls > odd_nuls.saturating_mul(4) {
        Some(Utf16Endian::Big)
    } else {
        None
    }
}

fn decode_utf16_hook_input(bytes: &[u8], endian: Utf16Endian) -> Result<String, String> {
    let chunks = bytes.chunks_exact(2);
    if !chunks.remainder().is_empty() {
        return Err("UTF-16 hook input has an odd byte length".to_string());
    }

    let code_units = chunks.map(|chunk| match endian {
        Utf16Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
        Utf16Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
    });

    String::from_utf16(&code_units.collect::<Vec<u16>>()).map_err(|e| e.to_string())
}
