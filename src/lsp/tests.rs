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
    let mut states = HashMap::new();
    run_and_publish_diagnostics(uri, text, &Options::default(), &mut states, &mut out).unwrap();
    let out_str = String::from_utf8(out).unwrap();

    // Verify it is a valid Content-Length and JSON-RPC structure
    assert!(out_str.contains("Content-Length:"));
    assert!(out_str.contains("textDocument/publishDiagnostics"));
    assert!(out_str.contains("BadOpe")); // Should report BadOpe error code
}

#[test]
fn test_lsp_requests() {
    // 1. Test word extraction
    let doc = "        move.l d0, my_const\nmy_const .equ $1234\n";
    assert_eq!(find_symbol_at_position(doc, 0, 19), Some("my_const"));
    assert_eq!(find_symbol_at_position(doc, 1, 3), Some("my_const"));

    // 2. Test formatting line
    assert_eq!(format_line("  move.l d0,a0  ; comment", 8), "        move.l  d0,a0 ; comment");
    assert_eq!(format_line("label:  nop", 8), "label:  nop");

    // 3. Test handle_request with states
    let uri = "file:///test.s";
    let mut states = HashMap::new();
    let mut out = Vec::new();
    run_and_publish_diagnostics(uri, doc, &Options::default(), &mut states, &mut out).unwrap();

    // Check hover request
    let hover_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::from(1),
        method: "textDocument/hover".to_string(),
        params: Some(serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 1, "character": 3 }
        })),
    };
    out.clear();
    handle_request(&hover_req, &states, &mut out).unwrap();
    let res_str = String::from_utf8(out.clone()).unwrap();
    assert!(res_str.contains("my_const"));
    assert!(res_str.contains("4660")); // $1234 = 4660 in decimal

    // Check definition request
    let def_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::from(2),
        method: "textDocument/definition".to_string(),
        params: Some(serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": 0, "character": 19 }
        })),
    };
    out.clear();
    handle_request(&def_req, &states, &mut out).unwrap();
    let res_str = String::from_utf8(out.clone()).unwrap();
    assert!(res_str.contains("uri"));
    assert!(res_str.contains("\"line\":1")); // defined on line index 1 (second line)

    // Check outline / documentSymbol request
    let symbol_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::from(3),
        method: "textDocument/documentSymbol".to_string(),
        params: Some(serde_json::json!({
            "textDocument": { "uri": uri }
        })),
    };
    out.clear();
    handle_request(&symbol_req, &states, &mut out).unwrap();
    let res_str = String::from_utf8(out.clone()).unwrap();
    assert!(res_str.contains("my_const"));

    // Check completion request
    let comp_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::from(4),
        method: "textDocument/completion".to_string(),
        params: Some(serde_json::json!({
            "textDocument": { "uri": uri }
        })),
    };
    out.clear();
    handle_request(&comp_req, &states, &mut out).unwrap();
    let res_str = String::from_utf8(out.clone()).unwrap();
    assert!(res_str.contains("move"));
    assert!(res_str.contains(".dc"));
    assert!(res_str.contains("my_const"));

    // Check formatting request
    let format_req = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: serde_json::Value::from(5),
        method: "textDocument/formatting".to_string(),
        params: Some(serde_json::json!({
            "textDocument": { "uri": uri },
            "options": { "tabSize": 8 }
        })),
    };
    out.clear();
    handle_request(&format_req, &states, &mut out).unwrap();
    let res_str = String::from_utf8(out.clone()).unwrap();
    assert!(res_str.contains("newText"));
}
