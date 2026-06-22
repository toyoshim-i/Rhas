//! PRNリストファイル生成（-p オプション）
//!
//! HAS060X互換のリストファイルフォーマット:
//! `NNNNN XXXXXXXX[* ]CCCCCCCCCCCCCCCCSSSSSSSSSSSS\n`
//! - NNNNN:  行番号 (5桁ゼロサプレス)
//! - XXXXXXXX: 16進アドレス (8桁)
//! - [ *]:   スペース（通常）または '*'（マクロ展開中）
//! - CCCC...: 機械語バイト列 (16文字 = 8バイト分)
//! - SSSS...: ソース行テキスト

/// PRNの1行エントリ
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PrnLine {
    pub line_num: u32,
    pub location: u32,
    pub section: u8,
    pub bytes: Vec<u8>, // 生成されたバイト列
    pub text: Vec<u8>,  // ソース行テキスト
    pub is_macro: bool,
}

/// PRNリストをバイト列として生成する
pub fn format_prn(
    lines: &[PrnLine],
    title: &[u8],
    subttl: &[u8],
    line_width: usize,
    code_width: usize,
    no_page_ff: bool,
    page_lines: usize,
) -> Vec<u8> {
    let mut out = Vec::new();
    let line_width = line_width.max(80);
    let code_width = code_width.max(4);
    let page_limit = if page_lines == u16::MAX as usize || page_lines < 10 {
        None
    } else {
        Some(page_lines)
    };
    let mut page_line_count = append_header(&mut out, title, subttl, line_width);

    for line in lines {
        if is_page_break_directive(&line.text) && !no_page_ff {
            out.push(0x0C);
            out.push(b'\n');
            page_line_count = 0;
        }
        if let Some(limit) = page_limit {
            if !no_page_ff && page_line_count >= limit {
                out.push(0x0C);
                out.push(b'\n');
                page_line_count = 0;
            }
        }
        page_line_count += format_prn_line(&mut out, line, title, line_width, code_width);
    }

    out
}

fn is_page_break_directive(text: &[u8]) -> bool {
    let mut p = 0usize;
    while p < text.len() && (text[p] == b' ' || text[p] == b'\t') {
        p += 1;
    }
    if p >= text.len() || text[p] == b';' {
        return false;
    }
    let mut end = p;
    while end < text.len() {
        let b = text[end];
        if b == b' ' || b == b'\t' || b == b';' {
            break;
        }
        end += 1;
    }
    if !text[p..end].eq_ignore_ascii_case(b".page") {
        return false;
    }

    let mut q = end;
    while q < text.len() && (text[q] == b' ' || text[q] == b'\t') {
        q += 1;
    }
    if q >= text.len() || text[q] == b';' {
        return true;
    }
    text[q] == b'+'
}

fn format_prn_line(
    out: &mut Vec<u8>,
    entry: &PrnLine,
    _title: &[u8],
    line_width: usize,
    code_width: usize,
) -> usize {
    // コードバイトのHEX文字列化（code_width文字まで）
    // 8バイト超の場合は継続行に分割
    let hex_chars: Vec<u8> = bytes_to_hex(&entry.bytes);
    let source_text = &entry.text;

    let mut code_offset = 0;
    let mut is_first = true;
    let mut emitted_lines = 0usize;

    loop {
        let code_end = (code_offset + code_width).min(hex_chars.len());
        let code_chunk = &hex_chars[code_offset..code_end];

        if is_first {
            // 行番号フィールド (5桁ゼロサプレス)
            let n = entry.line_num;
            if n == 0 {
                out.extend_from_slice(b"     ");
            } else {
                let s = format!("{:5}", n);
                out.extend_from_slice(s.as_bytes());
            }
            out.push(b' ');

            // アドレスフィールド (8桁16進)
            let addr_s = format!("{:08X}", entry.location);
            out.extend_from_slice(addr_s.as_bytes());
            out.push(b' ');

            // マクロ識別子
            out.push(if entry.is_macro { b'*' } else { b' ' });
        } else {
            // 継続行: 前置部をスペースで埋める (5+1+8+1+1 = 16文字)
            out.extend_from_slice(b"               ");
        }

        // コード部 (code_width文字、右側スペースパディング)
        out.extend_from_slice(code_chunk);
        let padding = code_width - code_chunk.len();
        for _ in 0..padding {
            out.push(b' ');
        }

        // ソース部（初回のみ、幅制限内）
        if is_first && !source_text.is_empty() {
            let max_src = line_width.saturating_sub(5 + 1 + 8 + 1 + 1 + code_width);
            let src_len = source_text.len().min(max_src);
            out.extend_from_slice(&source_text[..src_len]);
        }

        out.push(b'\n');
        emitted_lines += 1;

        code_offset = code_end;
        is_first = false;

        // まだコードバイトが残っている場合は継続行
        if code_offset >= hex_chars.len() {
            break;
        }
    }
    emitted_lines
}

fn append_header(out: &mut Vec<u8>, title: &[u8], subttl: &[u8], line_width: usize) -> usize {
    fn append_one(out: &mut Vec<u8>, prefix: &[u8], text: &[u8], width: usize) -> usize {
        if text.is_empty() {
            return 0;
        }
        let mut line = Vec::with_capacity(width);
        line.extend_from_slice(prefix);
        line.extend_from_slice(text);
        if line.len() > width {
            line.truncate(width);
        }
        out.extend_from_slice(&line);
        out.push(b'\n');
        1
    }

    append_one(out, b"; TITLE: ", title, line_width)
        + append_one(out, b"; SUBTTL: ", subttl, line_width)
}

/// バイト列を16進文字列に変換
fn bytes_to_hex(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let hi = (b >> 4) as usize;
        let lo = (b & 0xF) as usize;
        out.push(b"0123456789ABCDEF"[hi]);
        out.push(b"0123456789ABCDEF"[lo]);
    }
    out
}

#[cfg(test)]
mod tests;
