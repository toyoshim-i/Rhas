use crate::context::AssemblyContext;
use crate::error::BufferReporter;
use crate::options::Options;
use crate::pass::{pass1, pass2, pass3};
use crate::source::SourceBuf;
use crate::symbol::{Symbol, SymbolTable};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// キャッシュ用のドキュメント状態
struct DocumentState {
    text: String,
    sym: SymbolTable,
}

const MNEMONICS: &[&str] = &[
    "move", "moveq", "movem", "movep", "movea",
    "add", "addi", "addq", "addx", "adda",
    "sub", "subi", "subq", "subx", "suba",
    "mulu", "muls", "divu", "divs",
    "and", "andi", "or", "ori", "eor", "eori",
    "lsl", "lsr", "asl", "asr", "rol", "ror", "roxl", "roxr",
    "tst", "cmp", "cmpi", "cmpa", "cmpm",
    "clr", "neg", "negx", "not", "ext", "extb",
    "abcd", "sbcd", "nbcd", "pack", "unpk",
    "tas", "cas", "cas2", "chk", "chk2", "trap", "trapv",
    "jmp", "jsr", "rts", "rte", "rtr", "rtd",
    "nop", "reset", "stop", "dbra", "dbf",
    "bra", "bsr", "bchg", "bclr", "bset", "btst",
    "bcc", "bcs", "beq", "bge", "bgt", "bhi", "ble", "blt", "bmi", "bne", "bpl", "bvc", "bvs",
    "scc", "scs", "seq", "sge", "sgt", "shi", "sle", "slt", "smi", "sne", "spl", "svc", "svs",
];

const PSEUDOS: &[&str] = &[
    "dc", "ds", "dcb", "equ", "set", "reg", "include", "insert",
    "section", "offset", "offsym", "org", "fail", "pragma",
    "macro", "endm", "exitm", "local", "sizem",
    "if", "else", "elseif", "endif", "ifdef", "ifndef", "end",
];

/// LSPサーバーのメインループを開始する
pub fn start_lsp_server(opts: Options) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();

    let mut header_buf = String::new();
    let mut document_states: HashMap<String, DocumentState> = HashMap::new();

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
            handle_request(&req, &document_states, &mut stdout)?;
        } else if let Ok(notif) = serde_json::from_slice::<JsonRpcNotification>(&body_buf) {
            handle_notification(&notif, &opts, &mut document_states, &mut stdout)?;
        }
    }
}

fn handle_request<W: Write>(
    req: &JsonRpcRequest,
    states: &HashMap<String, DocumentState>,
    out: &mut W,
) -> io::Result<()> {
    match req.method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "capabilities": {
                    "textDocumentSync": 1, // Full sync
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "documentSymbolProvider": true,
                    "completionProvider": {
                        "resolveProvider": false,
                        "triggerCharacters": [".", ",", " "]
                    },
                    "documentFormattingProvider": true
                }
            });
            send_response(out, req.id.clone(), result)?;
        }
        "shutdown" => {
            send_response(out, req.id.clone(), serde_json::Value::Null)?;
        }
        "textDocument/hover" => {
            let mut result = serde_json::Value::Null;
            if let Some(ref params) = req.params {
                if let (Some(uri), Some(pos)) = (
                    params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()),
                    params.get("position"),
                ) {
                    if let (Some(line_idx), Some(char_idx)) = (
                        pos.get("line").and_then(|l| l.as_u64()),
                        pos.get("character").and_then(|c| c.as_u64()),
                    ) {
                        if let Some(state) = states.get(uri) {
                            if let Some(word) = find_symbol_at_position(&state.text, line_idx as usize, char_idx as usize) {
                                let sym_bytes = word.as_bytes();
                                let mut hover_text = String::new();

                                // ユーザー定義シンボル、レジスタ、命令の順で検索
                                if let Some(sym) = state.sym.lookup_sym(sym_bytes) {
                                    match sym {
                                        Symbol::Value { value, section, first, .. } => {
                                            hover_text = format!(
                                                "**Symbol**: `{}`\n\n- **Value**: {} (`${:X}`)\n- **Section**: {}\n- **Type**: {:?}",
                                                word, value, value, section, first
                                            );
                                        }
                                        Symbol::Macro { params, .. } => {
                                            let p_list: Vec<String> = params.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect();
                                            hover_text = format!(
                                                "**Macro**: `{}`\n\n- **Parameters**: `({})`",
                                                word, p_list.join(", ")
                                            );
                                        }
                                        Symbol::RegSym { .. } => {
                                            hover_text = format!("**Register Alias**: `{}`", word);
                                        }
                                        _ => {}
                                    }
                                } else if let Some(Symbol::Register { regno, .. }) = state.sym.lookup_reg(sym_bytes, crate::options::cpu::C000) {
                                    hover_text = format!("**Register**: `{}`\n- **Code**: {}", word, regno);
                                } else if let Some(Symbol::Opcode { handler, .. }) = state.sym.lookup_cmd(sym_bytes, crate::options::cpu::C000) {
                                    hover_text = format!("**Instruction**: `{}`\n- **Handler**: {:?}", word.to_ascii_lowercase(), handler);
                                }

                                if !hover_text.is_empty() {
                                    result = serde_json::json!({
                                        "contents": {
                                            "kind": "markdown",
                                            "value": hover_text
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
            send_response(out, req.id.clone(), result)?;
        }
        "textDocument/definition" => {
            let mut result = serde_json::Value::Null;
            if let Some(ref params) = req.params {
                if let (Some(uri), Some(pos)) = (
                    params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()),
                    params.get("position"),
                ) {
                    if let (Some(line_idx), Some(char_idx)) = (
                        pos.get("line").and_then(|l| l.as_u64()),
                        pos.get("character").and_then(|c| c.as_u64()),
                    ) {
                        if let Some(state) = states.get(uri) {
                            if let Some(word) = find_symbol_at_position(&state.text, line_idx as usize, char_idx as usize) {
                                // マクロ検索のために小文字にしたキーもフォールバック用に準備
                                let mut word_lower = word.to_string();
                                word_lower.make_ascii_lowercase();

                                let def_pos = state.sym.def_positions.get(word.as_bytes())
                                    .or_else(|| state.sym.def_positions.get(word_lower.as_bytes()));

                                if let Some(pos) = def_pos {
                                    let uri_str = if let Some(ref fp) = pos.filepath {
                                        let p_str = fp.to_string_lossy().replace("\\", "/");
                                        if p_str.starts_with('/') {
                                            format!("file://{}", p_str)
                                        } else {
                                            format!("file:///{}", p_str)
                                        }
                                    } else {
                                        uri.to_string()
                                    };
                                    let def_line = pos.line.saturating_sub(1);
                                    result = serde_json::json!({
                                        "uri": uri_str,
                                        "range": {
                                            "start": { "line": def_line, "character": 0 },
                                            "end": { "line": def_line, "character": 80 }
                                        }
                                    });
                                }
                            }
                        }
                    }
                }
            }
            send_response(out, req.id.clone(), result)?;
        }
        "textDocument/documentSymbol" => {
            let mut result = serde_json::json!([]);
            if let Some(ref params) = req.params {
                if let Some(uri) = params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()) {
                    if let Some(state) = states.get(uri) {
                        let mut symbols = Vec::new();
                        let doc_path = uri_to_path(uri);

                        for (name, sym) in state.sym.iter_user_syms() {
                            if let Some(pos) = state.sym.def_positions.get(name) {
                                // 同一ファイルで定義されたシンボルのみをアウトラインに追加
                                let matches_file = match (&pos.filepath, &doc_path) {
                                    (Some(ref fp), Some(ref dp)) => fp == dp,
                                    _ => true,
                                };
                                if !matches_file {
                                    continue;
                                }

                                let kind = match sym {
                                    Symbol::Value { first: crate::symbol::types::FirstDef::Set, .. } |
                                    Symbol::Value { first: crate::symbol::types::FirstDef::Offsym, .. } => 14, // Constant
                                    Symbol::Value { .. } => 13, // Variable
                                    Symbol::Macro { .. } => 12, // Function
                                    Symbol::RegSym { .. } => 13, // Variable
                                    _ => 13,
                                };

                                let name_str = String::from_utf8_lossy(name).into_owned();
                                let line = pos.line.saturating_sub(1);
                                symbols.push(serde_json::json!({
                                    "name": name_str,
                                    "kind": kind,
                                    "range": {
                                        "start": { "line": line, "character": 0 },
                                        "end": { "line": line, "character": 80 }
                                    },
                                    "selectionRange": {
                                        "start": { "line": line, "character": 0 },
                                        "end": { "line": line, "character": 80 }
                                    }
                                }));
                            }
                        }
                        result = serde_json::json!(symbols);
                    }
                }
            }
            send_response(out, req.id.clone(), result)?;
        }
        "textDocument/completion" => {
            let mut items = Vec::new();
            
            // 1. 命令一覧
            for &mnem in MNEMONICS {
                items.push(serde_json::json!({
                    "label": mnem,
                    "kind": 3, // Function
                    "detail": "Instruction"
                }));
            }

            // 2. 疑似命令一覧
            for &ps in PSEUDOS {
                items.push(serde_json::json!({
                    "label": format!(".{}", ps),
                    "kind": 14, // Keyword
                    "detail": "Pseudo Instruction"
                }));
            }

            // 3. 定義済みユーザーシンボル（パラメータからURIが取れればそのキャッシュを使う）
            if let Some(ref params) = req.params {
                if let Some(uri) = params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()) {
                    if let Some(state) = states.get(uri) {
                        for (name, sym) in state.sym.iter_user_syms() {
                            let name_str = String::from_utf8_lossy(name).into_owned();
                            let kind = match sym {
                                Symbol::Macro { .. } => 3, // Function
                                _ => 6, // Variable
                            };
                            items.push(serde_json::json!({
                                "label": name_str,
                                "kind": kind,
                                "detail": "User Symbol"
                            }));
                        }
                    }
                }
            }

            send_response(out, req.id.clone(), serde_json::json!(items))?;
        }
        "textDocument/formatting" => {
            let mut result = serde_json::json!([]);
            if let Some(ref params) = req.params {
                if let (Some(uri), Some(options)) = (
                    params.get("textDocument").and_then(|d| d.get("uri")).and_then(|u| u.as_str()),
                    params.get("options")
                ) {
                    if let Some(state) = states.get(uri) {
                        let tab_size = options.get("tabSize").and_then(|t| t.as_u64()).unwrap_or(8) as usize;
                        let mut formatted_lines = Vec::new();
                        for line in state.text.lines() {
                            formatted_lines.push(format_line(line, tab_size));
                        }
                        let formatted_text = formatted_lines.join("\n") + "\n";

                        // ドキュメント全体を置換するエディットを返す
                        let last_line = state.text.lines().count();
                        let last_char = state.text.lines().last().map(|l| l.len()).unwrap_or(0);

                        result = serde_json::json!([{
                            "range": {
                                "start": { "line": 0, "character": 0 },
                                "end": { "line": last_line, "character": last_char }
                            },
                            "newText": formatted_text
                        }]);
                    }
                }
            }
            send_response(out, req.id.clone(), result)?;
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

fn handle_notification<W: Write>(
    notif: &JsonRpcNotification,
    opts: &Options,
    states: &mut HashMap<String, DocumentState>,
    out: &mut W,
) -> io::Result<()> {
    match notif.method.as_str() {
        "exit" => {
            std::process::exit(0);
        }
        "textDocument/didOpen" => {
            if let Some(ref params) = notif.params {
                if let Some(doc) = params.get("textDocument") {
                    if let (Some(uri), Some(text)) = (doc.get("uri").and_then(|u| u.as_str()), doc.get("text").and_then(|t| t.as_str())) {
                        run_and_publish_diagnostics(uri, text, opts, states, out)?;
                    }
                }
            }
        }
        "textDocument/didChange" => {
            if let Some(ref params) = notif.params {
                if let (Some(doc), Some(changes)) = (params.get("textDocument"), params.get("contentChanges").and_then(|c| c.as_array())) {
                    if let (Some(uri), Some(change)) = (doc.get("uri").and_then(|u| u.as_str()), changes.first()) {
                        if let Some(text) = change.get("text").and_then(|t| t.as_str()) {
                            run_and_publish_diagnostics(uri, text, opts, states, out)?;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn run_and_publish_diagnostics<W: Write>(
    uri: &str,
    text: &str,
    opts: &Options,
    states: &mut HashMap<String, DocumentState>,
    out: &mut W,
) -> io::Result<()> {
    let path = uri_to_path(uri).unwrap_or_else(|| PathBuf::from("temp.s"));
    let content = text.as_bytes().to_vec();

    let mut ctx = AssemblyContext::new(opts.clone());

    let source_buf = SourceBuf::from_bytes(content, path);
    // Use include paths from Options
    let mut source = crate::source::SourceStack::new(source_buf, ctx.opts.include_paths.clone());
    let mut sym = SymbolTable::new(ctx.opts.sym_len8);
    let mut reporter = BufferReporter::new(2); // 警告レベル2

    // アセンブル実行 (Pass 1 - 2)
    let mut records = pass1::pass1(&mut source, &mut ctx, &mut sym, &mut reporter);
    pass2::pass2(&mut records, &mut sym);

    // Save/Update DocumentState in cache!
    states.insert(uri.to_string(), DocumentState {
        text: text.to_string(),
        sym,
    });

    // Re-lookup cached SymbolTable to run pass3 without moving sym
    if let Some(state) = states.get(uri) {
        let source_name = b"lsp_file".to_vec();
        let source_file = b"lsp_file.s".to_vec();
        pass3::pass3(
            &records,
            &state.sym,
            source_name,
            source_file,
            false,
            ctx.max_align,
            ctx.opts.all_xref,
            &mut reporter,
        );
    }

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

fn find_symbol_at_position(text: &str, line_idx: usize, char_idx: usize) -> Option<&str> {
    let line = text.lines().nth(line_idx)?;
    if char_idx >= line.len() {
        return None;
    }
    let start = line[..char_idx]
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '@')
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let end = line[char_idx..]
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '@')
        .map(|idx| char_idx + idx)
        .unwrap_or(line.len());
    if start < end {
        Some(&line[start..end])
    } else {
        None
    }
}

fn format_line(line: &str, tab_size: usize) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with(';') || trimmed.starts_with('*') {
        return line.to_string();
    }
    let mut pos = 0;
    let chars: Vec<char> = line.chars().collect();
    let mut label = String::new();
    let starts_with_label = !chars.first().map(|c| c.is_whitespace()).unwrap_or(true);
    if starts_with_label {
        while pos < chars.len() && !chars[pos].is_whitespace() {
            label.push(chars[pos]);
            pos += 1;
        }
    }
    while pos < chars.len() && chars[pos].is_whitespace() {
        pos += 1;
    }
    let mut mnemonic = String::new();
    while pos < chars.len() && !chars[pos].is_whitespace() && chars[pos] != ';' {
        mnemonic.push(chars[pos]);
        pos += 1;
    }
    while pos < chars.len() && chars[pos].is_whitespace() && chars[pos] != ';' {
        pos += 1;
    }
    let mut operands = String::new();
    let mut comment = String::new();
    let mut inside_string = false;
    let mut quote_char = ' ';
    while pos < chars.len() {
        let c = chars[pos];
        if !inside_string && c == ';' {
            comment = chars[pos..].iter().collect();
            break;
        }
        if c == '\'' || c == '"' {
            if inside_string {
                if c == quote_char {
                    inside_string = false;
                }
            } else {
                inside_string = true;
                quote_char = c;
            }
        }
        operands.push(c);
        pos += 1;
    }
    let operands_trimmed = operands.trim_end();
    let mut res = String::new();
    if !label.is_empty() {
        res.push_str(&label);
    }
    if !mnemonic.is_empty() {
        let target_col = if label.len() >= 8 {
            ((label.len() / tab_size) + 1) * tab_size
        } else {
            tab_size
        };
        let spaces = target_col.saturating_sub(res.len());
        res.push_str(&" ".repeat(spaces));
        res.push_str(&mnemonic);
    }
    if !operands_trimmed.is_empty() {
        let current_len = res.len();
        let target_col = if current_len >= 16 {
            ((current_len / tab_size) + 1) * tab_size
        } else {
            tab_size * 2
        };
        let spaces = target_col.saturating_sub(current_len);
        res.push_str(&" ".repeat(spaces));
        res.push_str(operands_trimmed);
    }
    if !comment.is_empty() {
        if !res.is_empty() {
            res.push(' ');
        }
        res.push_str(&comment);
    }
    res
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
