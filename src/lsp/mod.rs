use crate::context::AssemblyContext;
use crate::error::BufferReporter;
use crate::options::Options;
use crate::pass::{pass1, pass2, pass3};
use crate::source::SourceBuf;
use crate::symbol::SymbolTable;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;

#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: serde_json::Value,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: Option<serde_json::Value>,
}

/// LSPサーバーのメインループを開始する
pub fn start_lsp_server(opts: Options) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();

    let mut header_buf = String::new();

    loop {
        header_buf.clear();
        let mut content_length = 0;

        // Content-Length などのヘッダーを読み込む
        loop {
            let n = stdin_lock.read_line(&mut header_buf)?;
            if n == 0 {
                return Ok(()); // EOFで終了
            }
            if header_buf == "\r\n" || header_buf == "\n" {
                break; // 空行でヘッダー終了
            }
            let trimmed = header_buf.trim();
            if trimmed.to_lowercase().starts_with("content-length:") {
                if let Some(val_str) = trimmed.split(':').nth(1) {
                    content_length = val_str.trim().parse::<usize>().unwrap_or(0);
                }
            }
            header_buf.clear();
        }

        if content_length == 0 {
            continue;
        }

        let mut body_buf = vec![0u8; content_length];
        stdin_lock.read_exact(&mut body_buf)?;

        // JSON-RPC のパース
        if let Ok(req) = serde_json::from_slice::<JsonRpcRequest>(&body_buf) {
            handle_request(&req, &mut stdout)?;
        } else if let Ok(notif) = serde_json::from_slice::<JsonRpcNotification>(&body_buf) {
            handle_notification(&notif, &opts, &mut stdout)?;
        }
    }
}

fn handle_request<W: Write>(req: &JsonRpcRequest, out: &mut W) -> io::Result<()> {
    match req.method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "capabilities": {
                    "textDocumentSync": 1 // Full sync
                }
            });
            send_response(out, req.id.clone(), result)?;
        }
        "shutdown" => {
            send_response(out, req.id.clone(), serde_json::Value::Null)?;
        }
        _ => {
            let error = serde_json::json!({
                "code": -32601,
                "message": format!("Method not found: {}", req.method)
            });
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": req.id,
                "error": error
            });
            send_payload(out, &payload)?;
        }
    }
    Ok(())
}

fn handle_notification<W: Write>(notif: &JsonRpcNotification, opts: &Options, out: &mut W) -> io::Result<()> {
    match notif.method.as_str() {
        "exit" => {
            std::process::exit(0);
        }
        "textDocument/didOpen" => {
            if let Some(ref params) = notif.params {
                if let Some(doc) = params.get("textDocument") {
                    if let (Some(uri), Some(text)) = (doc.get("uri").and_then(|u| u.as_str()), doc.get("text").and_then(|t| t.as_str())) {
                        run_and_publish_diagnostics(uri, text, opts, out)?;
                    }
                }
            }
        }
        "textDocument/didChange" => {
            if let Some(ref params) = notif.params {
                if let (Some(doc), Some(changes)) = (params.get("textDocument"), params.get("contentChanges").and_then(|c| c.as_array())) {
                    if let (Some(uri), Some(change)) = (doc.get("uri").and_then(|u| u.as_str()), changes.first()) {
                        if let Some(text) = change.get("text").and_then(|t| t.as_str()) {
                            run_and_publish_diagnostics(uri, text, opts, out)?;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn run_and_publish_diagnostics<W: Write>(uri: &str, text: &str, opts: &Options, out: &mut W) -> io::Result<()> {
    let path = uri_to_path(uri).unwrap_or_else(|| PathBuf::from("temp.s"));
    let content = text.as_bytes().to_vec();

    let mut ctx = AssemblyContext::new(opts.clone());

    let source_buf = SourceBuf::from_bytes(content, path);
    // Use include paths from Options
    let mut source = crate::source::SourceStack::new(source_buf, ctx.opts.include_paths.clone());
    let mut sym = SymbolTable::new(ctx.opts.sym_len8);
    let mut reporter = BufferReporter::new(2); // 警告レベル2

    // アセンブル実行 (Pass 1 - 3)
    let mut records = pass1::pass1(&mut source, &mut ctx, &mut sym, &mut reporter);
    pass2::pass2(&mut records, &mut sym);

    let source_name = b"lsp_file".to_vec();
    let source_file = b"lsp_file.s".to_vec();
    pass3::pass3(
        &records,
        &sym,
        source_name,
        source_file,
        false,
        ctx.max_align,
        ctx.opts.all_xref,
        &mut reporter,
    );

    let lines: Vec<&str> = text.lines().collect();
    let mut diagnostics = Vec::new();

    // エラーの変換
    for err in reporter.errors {
        let line_idx = err.pos.line.saturating_sub(1) as usize;
        let line_str = lines.get(line_idx).copied().unwrap_or("");
        let (col_start, col_end) = get_cols(line_str, err.symbol.as_deref());

        let msg_display = format!("{}", err.code);
        let msg = if let Some(ref sym) = err.symbol {
            let sym_str = String::from_utf8_lossy(sym);
            msg_display.replacen("%s", &sym_str, 1)
        } else {
            msg_display
        };

        diagnostics.push(serde_json::json!({
            "range": {
                "start": { "line": line_idx, "character": col_start },
                "end": { "line": line_idx, "character": col_end }
            },
            "severity": 1, // Error
            "code": format!("{:?}", err.code),
            "source": "rhas",
            "message": msg
        }));
    }

    // 警告の変換
    for warn in reporter.warnings {
        let line_idx = warn.pos.line.saturating_sub(1) as usize;
        let line_str = lines.get(line_idx).copied().unwrap_or("");
        let (col_start, col_end) = get_cols(line_str, warn.symbol.as_deref());

        let msg_display = format!("{}", warn.code);
        let msg = if let Some(ref sym) = warn.symbol {
            let sym_str = String::from_utf8_lossy(sym);
            msg_display.replacen("%s", &sym_str, 1)
        } else {
            msg_display
        };

        diagnostics.push(serde_json::json!({
            "range": {
                "start": { "line": line_idx, "character": col_start },
                "end": { "line": line_idx, "character": col_end }
            },
            "severity": 2, // Warning
            "code": format!("{:?}", warn.code),
            "source": "rhas",
            "message": msg
        }));
    }

    let publish_params = serde_json::json!({
        "uri": uri,
        "diagnostics": diagnostics
    });

    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": publish_params
    });

    send_payload(out, &notification)
}

fn get_cols(line: &str, symbol: Option<&[u8]>) -> (usize, usize) {
    if let Some(s) = symbol {
        let sym_str = String::from_utf8_lossy(s);
        if let Some(idx) = line.find(sym_str.as_ref()) {
            return (idx, idx + sym_str.len());
        }
    }
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    let content_len = trimmed.trim_end().len();
    if content_len > 0 {
        (leading, leading + content_len)
    } else {
        (0, line.len())
    }
}

fn uri_to_path(uri: &str) -> Option<PathBuf> {
    if let Some(path_str) = uri.strip_prefix("file://") {
        let decoded = percent_decode(path_str)?;
        let path = if cfg!(windows) {
            decoded.strip_prefix("/").unwrap_or(&decoded).replace("/", "\\")
        } else {
            decoded
        };
        Some(PathBuf::from(path))
    } else {
        None
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let mut res = String::new();
    let mut bytes = s.as_bytes().iter();
    while let Some(&b) = bytes.next() {
        if b == b'%' {
            let h1 = (*bytes.next()? as char).to_digit(16)?;
            let h2 = (*bytes.next()? as char).to_digit(16)?;
            res.push((h1 * 16 + h2) as u8 as char);
        } else {
            res.push(b as char);
        }
    }
    Some(res)
}

fn send_response<W: Write>(out: &mut W, id: serde_json::Value, result: serde_json::Value) -> io::Result<()> {
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    });
    send_payload(out, &payload)
}

fn send_payload<W: Write>(out: &mut W, val: &serde_json::Value) -> io::Result<()> {
    let payload_str = serde_json::to_string(val)?;
    let msg = format!("Content-Length: {}\r\n\r\n{}", payload_str.len(), payload_str);
    out.write_all(msg.as_bytes())?;
    out.flush()?;
    Ok(())
}
