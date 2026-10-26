use super::*;

#[test]
fn redacts_python_ascii_json_escaped_tokens() {
    let token = "secret-é😀\u{7f}\\\"";
    let encoded = "secret-\\u00e9\\ud83d\\ude00\\u007f\\\\\\\"";
    assert_eq!(redact(encoded, token), "[REDACTED]");
    let unicode = serde_json::to_string(token).unwrap();
    assert_eq!(redact(&unicode[1..unicode.len() - 1], token), "[REDACTED]");
    assert_eq!(redact(token, token), "[REDACTED]");
}

#[test]
fn validates_and_sorts_pod_names() {
    assert_eq!(
        pod_names(
            r#"{"items":[{"metadata":{"name":"b"}},{"metadata":{"name":"a"}},{"metadata":{"name":"b"}}]}"#
        ),
        Ok(vec!["a".to_owned(), "b".to_owned()])
    );
    for bad in [
        "null",
        "[]",
        r#"{"items":{}}"#,
        r#"{"items":[null]}"#,
        r#"{"items":[{"metadata":{"name":4}}]}"#,
        r#"{"items":[{"metadata":{"name":"../bad"}}]}"#,
    ] {
        assert!(pod_names(bad).is_err(), "{bad}");
    }
}

#[test]
fn save_never_overwrites_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private.txt");
    save(&path, "first").unwrap();
    assert_eq!(
        save(&path, "replacement").unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "first");
}
