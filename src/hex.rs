pub fn encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    value
}

/// Decode `%XX` escapes in a URI path (e.g. `%20` -> space). Malformed
/// escapes are kept as-is; a result that is not UTF-8 returns the input.
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(&[high, low]) = bytes.get(i + 1..i + 3)
            && let (Some(high), Some(low)) = (hex_digit(high), hex_digit(low))
        {
            decoded.push(high << 4 | low);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| input.to_string())
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|digit| digit as u8)
}

#[cfg(test)]
mod tests {
    use super::percent_decode;

    #[test]
    fn percent_decode_decodes_escapes() {
        assert_eq!(
            percent_decode("/Users/test%20user/my%20project"),
            "/Users/test user/my project"
        );
        assert_eq!(percent_decode("/normal/path"), "/normal/path");
        assert_eq!(
            percent_decode("/path%2Fwith%2Fencoded"),
            "/path/with/encoded"
        );
        assert_eq!(percent_decode("/caf%C3%A9/project"), "/café/project");
        assert_eq!(percent_decode("/end%20"), "/end ");
    }

    #[test]
    fn percent_decode_keeps_malformed_escapes() {
        assert_eq!(percent_decode("/tmp/a%1é"), "/tmp/a%1é");
        assert_eq!(percent_decode("/tmp/%é1"), "/tmp/%é1");
        assert_eq!(percent_decode("/tmp/%+1"), "/tmp/%+1");
        assert_eq!(percent_decode("/tmp/100%"), "/tmp/100%");
        assert_eq!(percent_decode("/tmp/%2"), "/tmp/%2");
    }
}
