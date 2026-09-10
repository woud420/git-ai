use crate::model::jj_observer::paths::{decode, encode};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

#[test]
fn jj_observer_path_codec_preserves_exact_unix_bytes_without_filesystem_access() {
    for (bytes, hex) in [
        (b"/".as_slice(), "2f"),
        (b"/tmp/a\xff".as_slice(), "2f746d702f61ff"),
        (b"/a/../b\n".as_slice(), "2f612f2e2e2f620a"),
        (b"/file:literal".as_slice(), "2f66696c653a6c69746572616c"),
    ] {
        let path = PathBuf::from(OsString::from_vec(bytes.to_vec()));
        assert_eq!(encode(&path).unwrap(), hex);
        assert_eq!(decode(hex).unwrap().as_os_str().as_bytes(), bytes);
    }
}

#[test]
fn jj_observer_path_codec_accepts_exact_decoded_limit_and_rejects_one_more() {
    let maximum = 64 * 1024;
    let path = PathBuf::from(format!("/{}", "a".repeat(maximum - 1)));
    let hex = format!("2f{}", "61".repeat(maximum - 1));
    assert_eq!(encode(&path).unwrap(), hex);
    assert_eq!(decode(&hex).unwrap(), path);
    assert_eq!(hex.len(), 2 * maximum);
    assert!(encode(Path::new(&format!("/{}", "a".repeat(maximum)))).is_err());
    assert!(decode(&format!("{hex}61")).is_err());
}

#[test]
fn jj_observer_path_codec_rejects_noncanonical_hex_and_unsafe_decoded_paths() {
    for hex in [
        "",
        "2",
        "2f0",
        "2F",
        "2g",
        "zz",
        "2f 61",
        "2fé",
        "2f00",
        "00",
        "61",
        "2e2f61",
        "66696c653a61",
        "3a6d656d6f72793a",
    ] {
        assert!(decode(hex).is_err(), "accepted {hex:?}");
    }
}

#[test]
fn jj_observer_path_codec_rejects_empty_relative_and_nul_input() {
    for bytes in [
        b"".as_slice(),
        b"relative".as_slice(),
        b"./file:literal".as_slice(),
        b":memory:".as_slice(),
        b"file:/tmp/a?immutable=1".as_slice(),
        b"/tmp/a\0b".as_slice(),
    ] {
        let path = PathBuf::from(OsString::from_vec(bytes.to_vec()));
        assert!(encode(&path).is_err(), "accepted {bytes:?}");
    }
}
