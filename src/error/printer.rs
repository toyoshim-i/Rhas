use super::codes::{warn_default_level, warn_message, ErrorCode, WarnCode};
use super::context::{ErrorContext, SourcePos, WarnContext};
use crate::utils;
use std::io::Write;

/// アセンブラのエラー出力（error.s の printerr に相当）
///
/// フォーマット: `<filename>  <linenum>: Error: <message>\n`
pub fn print_error(out: &mut dyn Write, pos: &SourcePos, code: ErrorCode, sym: Option<&[u8]>) {
    let msg = format_message(code.message(), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Error: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// ErrorContext 版エラー出力（型安全性改善版）
///
/// フォーマット: `<filename>  <linenum>: Error: <message>\n`
pub fn print_error_context(out: &mut dyn Write, ctx: &ErrorContext) {
    let sym_ref = ctx.symbol.as_ref().map(|s| s.as_slice());
    print_error(out, &ctx.pos, ctx.code, sym_ref);
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
    let msg = format_message(warn_message(code), sym);
    let _ = writeln!(
        out,
        "{} {:6}: Warning: {}",
        pos.filename_display(),
        pos.line,
        msg
    );
}

/// WarnContext 版ワーニング出力（型安全性改善版）
pub fn print_warning_context(out: &mut dyn Write, ctx: &WarnContext, warn_level: u8) {
    let sym_ref = ctx.symbol.as_ref().map(|s| s.as_slice());
    print_warning(out, &ctx.pos, ctx.code, sym_ref, warn_level);
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
