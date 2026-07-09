use super::*;

#[test]
fn test_percent_decode() {
    assert_eq!(percent_decode("/a/b/c"), Some("/a/b/c".to_string()));
    assert_eq!(percent_decode("/a/b%20c/d"), Some("/a/b c/d".to_string()));
    assert_eq!(percent_decode("/a/b%25c"), Some("/a/b%c".to_string()));
}

#[test]
fn test_uri_to_path() {
    let uri = "file:///home/user/project/file.s";
    let path = uri_to_path(uri).unwrap();
    if cfg!(windows) {
        assert_eq!(path, PathBuf::from("home\\user\\project\\file.s"));
    } else {
        assert_eq!(path, PathBuf::from("/home/user/project/file.s"));
    }
}

#[test]
fn test_get_cols() {
    // 1. Symbol found in line
    let line = "        move.b d0, a0";
    let (start, end) = get_cols(line, Some(b"d0"));
    assert_eq!(start, 15);
    assert_eq!(end, 17);

    // 2. Symbol not found or none - fallbacks to trimmed range
    let (start, end) = get_cols("   nop   ", None);
    assert_eq!(start, 3);
    assert_eq!(end, 6);
}

#[test]
fn test_run_diagnostics() {
    let uri = "file:///test.s";
    let text = "        .68000\n        invalid_op\n";
    let mut out = Vec::new();
    run_and_publish_diagnostics(uri, text, &Options::default(), &mut out).unwrap();
    let out_str = String::from_utf8(out).unwrap();

    // Verify it is a valid Content-Length and JSON-RPC structure
    assert!(out_str.contains("Content-Length:"));
    assert!(out_str.contains("textDocument/publishDiagnostics"));
    assert!(out_str.contains("BadOpe")); // Should report BadOpe error code
}
