//! Pure syntax checks for full Git object IDs.

/// Return whether `value` is a complete SHA-1 or SHA-256 object ID.
///
/// This is a syntax check only; it does not verify that the object exists.
pub fn is_full_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

/// Return whether `value` is a full-width all-zero object ID.
pub fn is_zero_oid(value: &str) -> bool {
    is_full_oid(value) && value.as_bytes().iter().all(|byte| *byte == b'0')
}

/// Return whether `value` is a full-width, non-zero object ID.
pub fn is_non_zero_oid(value: &str) -> bool {
    is_full_oid(value) && !is_zero_oid(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_full_zero_and_non_zero_oids() {
        let sha1_lower = "0123456789abcdef0123456789abcdef01234567";
        let sha1_upper = "0123456789ABCDEF0123456789ABCDEF01234567";
        let sha1_mixed = "0123456789aBcDeF0123456789AbCdEf01234567";
        let sha256_lower = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let sha256_upper = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        let sha256_mixed = "0123456789aBcDeF0123456789AbCdEf0123456789aBcDeF0123456789AbCdEf";
        let zero_sha1 = "0".repeat(40);
        let zero_sha256 = "0".repeat(64);

        for oid in [
            sha1_lower,
            sha1_upper,
            sha1_mixed,
            sha256_lower,
            sha256_upper,
            sha256_mixed,
        ] {
            assert!(is_full_oid(oid), "expected full OID: {oid}");
            assert!(!is_zero_oid(oid), "expected non-zero OID: {oid}");
            assert!(is_non_zero_oid(oid), "expected non-zero OID: {oid}");
        }

        for oid in [&zero_sha1, &zero_sha256] {
            assert!(is_full_oid(oid), "expected full OID: {oid}");
            assert!(is_zero_oid(oid), "expected zero OID: {oid}");
            assert!(!is_non_zero_oid(oid), "expected zero OID: {oid}");
        }
    }

    #[test]
    fn rejects_non_full_or_non_hex_values() {
        let cases = [
            String::new(),
            "a".repeat(4),
            "a".repeat(39),
            "a".repeat(41),
            "a".repeat(63),
            "a".repeat(65),
            format!("{}g", "a".repeat(39)),
            format!("{} ", "a".repeat(39)),
            format!("{}\n", "a".repeat(39)),
            format!("{}é", "a".repeat(39)),
        ];

        for value in cases {
            assert!(!is_full_oid(&value), "unexpected full OID: {value:?}");
            assert!(!is_zero_oid(&value), "unexpected zero OID: {value:?}");
            assert!(
                !is_non_zero_oid(&value),
                "unexpected non-zero OID: {value:?}"
            );
        }
    }
}
