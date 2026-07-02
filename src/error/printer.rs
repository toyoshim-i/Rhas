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
    use std::io::IsTerminal;
    let use_color = std::io::stderr().is_terminal();
    let c_red = if use_color { "\x1b[1;31m" } else { "" };
    let c_bold = if use_color { "\x1b[1;37m" } else { "" };
    let c_blue = if use_color { "\x1b[1;36m" } else { "" };
    let c_reset = if use_color { "\x1b[0m" } else { "" };

    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(out, "{}error[{:?}]: {}{}{}", c_red, code, c_bold, msg, c_reset);
    let filename_str = utils::bytes_to_string(&pos.filename);
    let _ = writeln!(out, "  {}-->{} {}:{}", c_blue, c_reset, filename_str, pos.line);

    if let Some(ref path) = pos.filepath {
        let (prev, curr, next) = get_source_lines(path, pos.line);
        if let Some(line) = curr {
            let line_num_width = (pos.line + 1).to_string().len().max(2);

            let _ = writeln!(out, "{:>width$} |{}", "", c_blue, width = line_num_width);

            // 前のコンテキスト行
            if let Some(prev_line) = prev {
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}",
                    c_blue,
                    pos.line - 1,
                    c_reset,
                    prev_line,
                    width = line_num_width
                );
            }

            // エラー該当行
            let _ = writeln!(
                out,
                "{}{:>width$} |{} {}",
                c_blue,
                pos.line,
                c_reset,
                line,
                width = line_num_width
            );

            // 下線表示
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
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}{}{}",
                    c_blue, "", c_reset, spaces, c_red, carets, width = line_num_width
                );
            }

            // 次のコンテキスト行
            if let Some(next_line) = next {
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}",
                    c_blue,
                    pos.line + 1,
                    c_reset,
                    next_line,
                    width = line_num_width
                );
            }

            let _ = writeln!(out, "{:>width$} |{}", "", c_blue, width = line_num_width);
        }
    }
    // Reset any lingering styles
    if use_color {
        let _ = write!(out, "\x1b[0m");
    }
}

/// モダンな警告出力（rustc 風）
fn print_modern_warning(out: &mut dyn Write, pos: &SourcePos, code: WarnCode, sym: Option<&[u8]>, warn_level: u8) {
    if warn_level < warn_default_level(code) {
        return;
    }
    use std::io::IsTerminal;
    let use_color = std::io::stderr().is_terminal();
    let c_yellow = if use_color { "\x1b[1;33m" } else { "" };
    let c_bold = if use_color { "\x1b[1;37m" } else { "" };
    let c_blue = if use_color { "\x1b[1;36m" } else { "" };
    let c_reset = if use_color { "\x1b[0m" } else { "" };

    let msg = format_message(&code.to_string(), sym);
    let _ = writeln!(out, "{}warning[{:?}]: {}{}{}", c_yellow, code, c_bold, msg, c_reset);
    let filename_str = utils::bytes_to_string(&pos.filename);
    let _ = writeln!(out, "  {}-->{} {}:{}", c_blue, c_reset, filename_str, pos.line);

    if let Some(ref path) = pos.filepath {
        let (prev, curr, next) = get_source_lines(path, pos.line);
        if let Some(line) = curr {
            let line_num_width = (pos.line + 1).to_string().len().max(2);

            let _ = writeln!(out, "{:>width$} |{}", "", c_blue, width = line_num_width);

            // 前のコンテキスト行
            if let Some(prev_line) = prev {
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}",
                    c_blue,
                    pos.line - 1,
                    c_reset,
                    prev_line,
                    width = line_num_width
                );
            }

            // 警告該当行
            let _ = writeln!(
                out,
                "{}{:>width$} |{} {}",
                c_blue,
                pos.line,
                c_reset,
                line,
                width = line_num_width
            );

            // 下線表示
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
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}{}{}",
                    c_blue, "", c_reset, spaces, c_yellow, carets, width = line_num_width
                );
            }

            // 次のコンテキスト行
            if let Some(next_line) = next {
                let _ = writeln!(
                    out,
                    "{}{:>width$} |{} {}",
                    c_blue,
                    pos.line + 1,
                    c_reset,
                    next_line,
                    width = line_num_width
                );
            }

            let _ = writeln!(out, "{:>width$} |{}", "", c_blue, width = line_num_width);
        }
    }
    // Reset any lingering styles
    if use_color {
        let _ = write!(out, "\x1b[0m");
    }
}

/// ファイルから前後のコンテキスト行を含めて（文字エンコーディングに依らず安全に）取得する
fn get_source_lines(path: &std::path::Path, target_line: u32) -> (Option<String>, Option<String>, Option<String>) {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(_) => return (None, None, None),
    };
    let mut prev = None;
    let mut curr = None;
    let mut next = None;

    let mut line_count = 0;
    let mut start = 0;
    for i in 0..=data.len() {
        let is_eof = i == data.len();
        let is_nl = !is_eof && data[i] == b'\n';
        if is_nl || is_eof {
            line_count += 1;
            let mut end = i;
            if end > start && data[end - 1] == b'\r' {
                end -= 1;
            }
            let line_str = String::from_utf8_lossy(&data[start..end]).into_owned();

            if target_line >= 2 && line_count == target_line - 1 {
                prev = Some(line_str.clone());
            }
            if line_count == target_line {
                curr = Some(line_str.clone());
            }
            if line_count == target_line + 1 {
                next = Some(line_str);
                break;
            }
            start = i + 1;
        }
    }
    (prev, curr, next)
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
