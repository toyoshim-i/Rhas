use super::codes::{warn_default_level, ErrorCode, WarnCode};
use super::context::{ErrorContext, SourcePos, WarnContext};
use crate::utils;
use std::io::Write;

/// アセンブラのエラー出力（error.s の printerr に相当）
///
/// フォーマット: `<filename>  <linenum>: Error: <message>\n`
pub fn print_error(out: &mut dyn Write, pos: &SourcePos, code: ErrorCode, sym: Option<&[u8]>) {
    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Error: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// ErrorContext 版エラー出力（型安全性改善版）
pub fn print_error_context(out: &mut dyn Write, ctx: &ErrorContext<'_>, compat_error_format: bool) {
    if compat_error_format {
        print_error(out, ctx.pos, ctx.code, ctx.symbol);
    } else {
        print_modern_error(out, ctx.pos, ctx.code, ctx.symbol);
    }
}

/// ワーニング出力
pub fn print_warning(
    out: &mut dyn Write,
    pos: &SourcePos,
    code: WarnCode,
    sym: Option<&[u8]>,
    warn_level: u8,
) {
    if warn_level < warn_default_level(code) {
        return;
    }
    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Warning: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// WarnContext 版ワーニング出力（型安全性改善版）
pub fn print_warning_context(out: &mut dyn Write, ctx: &WarnContext<'_>, warn_level: u8, compat_error_format: bool) {
    if compat_error_format {
        print_warning(out, ctx.pos, ctx.code, ctx.symbol, warn_level);
    } else {
        print_modern_warning(out, ctx.pos, ctx.code, ctx.symbol, warn_level);
    }
}

/// モダンなエラー出力（rustc 風）
fn print_modern_error(out: &mut dyn Write, pos: &SourcePos, code: ErrorCode, sym: Option<&[u8]>) {
    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(out, "error[{:?}]: {}", code, msg);
    let filename_str = utils::bytes_to_string(&pos.filename);
    let _ = writeln!(out, "  --> {}:{}", filename_str, pos.line);

    if let Some(ref path) = pos.filepath {
        if let Some(line) = get_source_line(path, pos.line) {
            let _ = writeln!(out, "   |");
            let _ = writeln!(out, "{:2} | {}", pos.line, line);

            let mut underline_start = None;
            let mut underline_len = 0;
            if let Some(s) = sym {
                let sym_str = utils::bytes_to_string(s);
                if let Some(idx) = line.find(&sym_str) {
                    underline_start = Some(idx);
                    underline_len = sym_str.len();
                }
            }
            if underline_start.is_none() {
                let trimmed = line.trim_start();
                let leading = line.len() - trimmed.len();
                let content_len = trimmed.trim_end().len();
                if content_len > 0 {
                    underline_start = Some(leading);
                    underline_len = content_len;
                }
            }

            if let Some(start_idx) = underline_start {
                let mut spaces = String::new();
                for (idx, c) in line.chars().enumerate() {
                    if idx >= start_idx {
                        break;
                    }
                    if c == '\t' {
                        spaces.push('\t');
                    } else {
                        spaces.push(' ');
                    }
                }
                let carets = "^".repeat(underline_len);
                let _ = writeln!(out, "   | {}{}", spaces, carets);
            }
        }
    }
}

/// モダンな警告出力（rustc 風）
fn print_modern_warning(out: &mut dyn Write, pos: &SourcePos, code: WarnCode, sym: Option<&[u8]>, warn_level: u8) {
    if warn_level < warn_default_level(code) {
        return;
    }
    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(out, "warning[{:?}]: {}", code, msg);
    let filename_str = utils::bytes_to_string(&pos.filename);
    let _ = writeln!(out, "  --> {}:{}", filename_str, pos.line);

    if let Some(ref path) = pos.filepath {
        if let Some(line) = get_source_line(path, pos.line) {
            let _ = writeln!(out, "   |");
            let _ = writeln!(out, "{:2} | {}", pos.line, line);

            let mut underline_start = None;
            let mut underline_len = 0;
            if let Some(s) = sym {
                let sym_str = utils::bytes_to_string(s);
                if let Some(idx) = line.find(&sym_str) {
                    underline_start = Some(idx);
                    underline_len = sym_str.len();
                }
            }
            if underline_start.is_none() {
                let trimmed = line.trim_start();
                let leading = line.len() - trimmed.len();
                let content_len = trimmed.trim_end().len();
                if content_len > 0 {
                    underline_start = Some(leading);
                    underline_len = content_len;
                }
            }

            if let Some(start_idx) = underline_start {
                let mut spaces = String::new();
                for (idx, c) in line.chars().enumerate() {
                    if idx >= start_idx {
                        break;
                    }
                    if c == '\t' {
                        spaces.push('\t');
                    } else {
                        spaces.push(' ');
                    }
                }
                let carets = "^".repeat(underline_len);
                let _ = writeln!(out, "   | {}{}", spaces, carets);
            }
        }
    }
}

/// ファイルから特定の行番号のコンテンツを（文字エンコーディングに依らず安全に）取得する
fn get_source_line(path: &std::path::Path, line_num: u32) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let mut line_count = 0;
    let mut start = 0;
    for i in 0..=data.len() {
        let is_eof = i == data.len();
        let is_nl = !is_eof && data[i] == b'\n';
        if is_nl || is_eof {
            line_count += 1;
            if line_count == line_num {
                let mut end = i;
                if end > start && data[end - 1] == b'\r' {
                    end -= 1;
                }
                let bytes = &data[start..end];
                return Some(String::from_utf8_lossy(bytes).into_owned());
            }
            start = i + 1;
        }
    }
    None
}

/// `%s` を sym で置換する
fn format_message(template: &str, sym: Option<&[u8]>) -> String {
    if let Some(s) = sym {
        let sym_str = utils::bytes_to_string(s);
        template.replacen("%s", &sym_str, 1)
    } else {
        template.to_string()
    }
}
