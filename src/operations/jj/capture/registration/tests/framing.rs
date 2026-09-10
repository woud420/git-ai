use super::*;

#[test]
fn seal_has_one_closed_ascii_representation() {
    let expected = expected_seal(SOURCE);
    assert_eq!(expected.len(), 141);
    let parsed = SourceSeal::parse(&expected).unwrap();
    assert_eq!(parsed.source_id(), SOURCE);
    assert_eq!(parsed.bytes(), expected);
    assert_eq!(SourceSeal::new(SOURCE).unwrap().bytes(), expected);
    assert!(SourceSeal::new(&"0".repeat(64)).is_ok());
}

#[test]
fn seal_rejects_noncanonical_or_extensible_framing() {
    let valid = expected_seal(SOURCE);
    let mut invalid = vec![
        Vec::new(),
        valid[..valid.len() - 1].to_vec(),
        [valid.as_slice(), b"\n"].concat(),
        [valid.as_slice(), b"extension=1\n"].concat(),
        [b"\xef\xbb\xbf".as_slice(), valid.as_slice()].concat(),
        String::from_utf8(valid.clone()).unwrap().replace('\n', "\r\n").into_bytes(),
        String::from_utf8(valid.clone()).unwrap().replace("/v1\n", "/v2\n").into_bytes(),
        String::from_utf8(valid.clone()).unwrap().replace("0.45.1", "0.45.2").into_bytes(),
        format!("git-ai/jj/source-seal/v1\nreader_profile=jj-simple-op-store/0.45.1\nsource_id={SOURCE}\n").into_bytes(),
        vec![b'x'; 1024],
        vec![b'x'; 1025],
    ];
    let mut nul = valid;
    nul[25] = 0;
    invalid.push(nul);
    for bytes in invalid {
        assert!(SourceSeal::parse(&bytes).is_err(), "accepted {bytes:?}");
    }
}

#[test]
fn seal_rejects_noncanonical_namespace_ids() {
    for id in [
        String::new(),
        SOURCE[..63].to_owned(),
        format!("{SOURCE}0"),
        SOURCE.to_uppercase(),
        "g".repeat(64),
        format!("{}\n", &SOURCE[..63]),
    ] {
        assert!(SourceSeal::new(&id).is_err());
        assert!(SourceSeal::parse(&expected_seal(&id)).is_err());
    }
}
