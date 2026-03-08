//! Pseudo-instruction handlers for macro and repetition
//!
//! Handles: .macro, .endm, .rept, .irp, .irpc
//! Complex control flow for assembler-time macro expansion and repetitive code generation.

use crate::expr::parse_expr;
use crate::error::ErrorCode;
use crate::source::SourceStack;
use crate::symbol::Symbol;
use crate::symbol::types::InsnHandler;
use super::super::temp::TempRecord;
use crate::pass::pass1::{
    P1Ctx, skip_spaces, read_ident,
    parse_macro_params, parse_macro_args, parse_string_or_ident,
    collect_macro_body, expand_macro_body,
};

/// Helper to parse repetition count
pub fn parse_repeat_count(value: u32) -> u32 {
    if value == 0 { 0 } else { value }
}

/// Helper to track macro nesting depth
pub fn check_macro_nesting(current_depth: u16, max_depth: u16) -> bool {
    current_depth < max_depth
}

/// Dispatch macro-related pseudo instructions from pass1
pub fn handle_macro(
    handler: InsnHandler,
    label: Option<Vec<u8>>,
    line: &[u8],
    pos: &mut usize,
    source: &mut SourceStack,
    p1: &mut P1Ctx<'_>,
    records: &mut Vec<TempRecord>,
) {
    match handler {
        InsnHandler::MacroDef => {
            let mac_name = label.unwrap_or_default();
            if mac_name.is_empty() {
                p1.error_code(ErrorCode::NoSymMacro, None);
                return;
            }
            let params = parse_macro_params(line, pos);
            let (template, local_count) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            let sym = Symbol::Macro { params, local_count, template };
            p1.sym.define_macro(mac_name, sym);
        }
        InsnHandler::Rept => {
            let count = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value as u32).unwrap_or(0)
            } else { 0 };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &[]);
            for _ in 0..count {
                let lb = p1.next_local_base();
                expand_macro_body(&body, &[], &[], lb, records, p1, source);
            }
        }
        InsnHandler::Irp => {
            skip_spaces(line, pos);
            let param_name = read_ident(line, pos);
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] == b',' { *pos += 1; }
            let args = parse_macro_args(line, pos);
            let params = if param_name.is_empty() { vec![] } else { vec![param_name] };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            for arg in &args {
                let lb = p1.next_local_base();
                expand_macro_body(&body, &params, std::slice::from_ref(arg), lb, records, p1, source);
            }
        }
        InsnHandler::Irpc => {
            skip_spaces(line, pos);
            let param_name = read_ident(line, pos);
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] == b',' { *pos += 1; }
            skip_spaces(line, pos);
            let s = parse_string_or_ident(line, pos);
            let params = if param_name.is_empty() { vec![] } else { vec![param_name] };
            let (body, _) = collect_macro_body(source, p1.sym, p1.ctx, &params);
            for &ch in &s {
                let arg = vec![ch];
                let lb = p1.next_local_base();
                expand_macro_body(&body, &params, std::slice::from_ref(&arg), lb, records, p1, source);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repeat_count_zero() {
        assert_eq!(parse_repeat_count(0), 0);
    }

    #[test]
    fn test_parse_repeat_count_nonzero() {
        assert_eq!(parse_repeat_count(10), 10);
        assert_eq!(parse_repeat_count(1), 1);
    }

    #[test]
    fn test_check_macro_nesting_within() {
        assert!(check_macro_nesting(2, 10));
        assert!(check_macro_nesting(9, 10));
    }

    #[test]
    fn test_check_macro_nesting_full() {
        assert!(!check_macro_nesting(10, 10));
        assert!(!check_macro_nesting(11, 10));
    }

    #[test]
    fn test_check_macro_nesting_zero() {
        assert!(check_macro_nesting(0, 1));
    }
}
