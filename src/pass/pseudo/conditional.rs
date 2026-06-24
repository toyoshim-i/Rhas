//! Pseudo-instruction handlers for conditional assembly
//!
//! Handles: .if, .iff, .ifdef, .ifndef, .else, .elseif, .endif
//! Complex control flow for assembler-time conditional compilation.

// HAS060互換の条件アセンブリ疑似命令の複雑な制御フローや関連ハンドラ群の一部が、
// 現在の単純化されたアセンブルパス構成において未参照となっている警告を抑制するために付与しています。
#![allow(dead_code)]

use crate::expr::parse_expr;
use crate::symbol::SymbolTable;
use crate::symbol::types::InsnHandler;

/// Helper to read identifier from line starting at pos
pub fn read_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
    while *pos < line.len() && line[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    let start = *pos;
    while *pos < line.len() && (line[*pos].is_ascii_alphanumeric() || line[*pos] == b'_') {
        *pos += 1;
    }
    line[start..*pos].to_vec()
}

/// Helper to skip whitespace
pub fn skip_spaces(line: &[u8], pos: &mut usize) {
    while *pos < line.len() && line[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
}

/// Evaluate condition expression result (non-zero = true)
/// Returns (success, value) where value is true if condition matches
pub fn evaluate_condition_expr(line: &[u8], pos: &mut usize) -> (bool, bool) {
    skip_spaces(line, pos);
    if parse_expr(line, pos).is_ok() {
        // Note: eval_const needs P1Ctx context - this returns (can_evaluate, parsed_ok)
        (true, true)  // Placeholder: actual eval deferred to caller
    } else {
        (false, false)
    }
}

/// Check if symbol is defined
pub fn is_symbol_defined(line: &[u8], pos: &mut usize, sym: &SymbolTable) -> bool {
    let name = read_ident(line, pos);
    !name.is_empty() && sym.lookup_sym(&name).is_some()
}

/// Helper to safely set if_matched array element
pub fn set_if_matched(if_matched: &mut [bool], idx: usize, value: bool) {
    if idx < if_matched.len() {
        if_matched[idx] = value;
    }
}

/// Called when parser is currently skipping lines due to a false conditional.
/// Only the conditional-related handlers need to be processed in skip mode.
pub fn handle_skip(
    handler: Option<InsnHandler>,
    line: &[u8],
    pos: &mut usize,
    p1: &mut crate::pass::pass1::P1Ctx<'_>,
) {
    match handler {
        Some(InsnHandler::If | InsnHandler::Iff | InsnHandler::Ifdef | InsnHandler::Ifndef) => {
            p1.if_nest += 1;
            set_if_matched(&mut p1.if_matched, p1.if_nest as usize, false);
        }
        Some(InsnHandler::Else) => {
            if p1.skip_nest == p1.if_nest {
                let idx = p1.if_nest as usize;
                let already = idx < p1.if_matched.len() && p1.if_matched[idx];
                if !already {
                    // we can resume assembly inside .else
                    p1.is_skip = false;
                    set_if_matched(&mut p1.if_matched, idx, true);
                }
            }
        }
        Some(InsnHandler::Elseif) => {
            if p1.skip_nest == p1.if_nest {
                let idx = p1.if_nest as usize;
                let already = idx < p1.if_matched.len() && p1.if_matched[idx];
                if !already {
                    skip_spaces(line, pos);
                    let cond = if let Ok(rpn) = parse_expr(line, pos) {
                        p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(false)
                    } else { false };
                    if cond {
                        p1.is_skip = false;
                        set_if_matched(&mut p1.if_matched, idx, true);
                    }
                }
            }
        }
        Some(InsnHandler::Endif) => {
            if p1.if_nest > 0 {
                if p1.skip_nest == p1.if_nest {
                    p1.is_skip = false;
                    p1.skip_nest = 0;
                }
                let idx = p1.if_nest as usize;
                set_if_matched(&mut p1.if_matched, idx, false);
                p1.if_nest -= 1;
            }
        }
        _ => {}
    }
}

/// Normal processing of conditional directives when not currently skipping
pub fn handle_conditional(
    handler: InsnHandler,
    line: &[u8],
    pos: &mut usize,
    p1: &mut crate::pass::pass1::P1Ctx<'_>,
) {
    match handler {
        InsnHandler::If => {
            p1.if_nest += 1;
            let idx = p1.if_nest as usize;
            set_if_matched(&mut p1.if_matched, idx, false);
            skip_spaces(line, pos);
            let cond = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(false)
            } else { false };
            if cond {
                set_if_matched(&mut p1.if_matched, idx, true);
            } else {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Iff => {
            p1.if_nest += 1;
            let idx = p1.if_nest as usize;
            set_if_matched(&mut p1.if_matched, idx, false);
            skip_spaces(line, pos);
            let cond = if let Ok(rpn) = parse_expr(line, pos) {
                p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(false)
            } else { false };
            if !cond {
                set_if_matched(&mut p1.if_matched, idx, true);
            } else {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Ifdef => {
            p1.if_nest += 1;
            let idx = p1.if_nest as usize;
            set_if_matched(&mut p1.if_matched, idx, false);
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            let defined = !name.is_empty() && p1.sym.lookup_sym(&name).is_some();
            if defined {
                set_if_matched(&mut p1.if_matched, idx, true);
            } else {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Ifndef => {
            p1.if_nest += 1;
            let idx = p1.if_nest as usize;
            set_if_matched(&mut p1.if_matched, idx, false);
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            let defined = !name.is_empty() && p1.sym.lookup_sym(&name).is_some();
            if !defined {
                set_if_matched(&mut p1.if_matched, idx, true);
            } else {
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Else => {
            if p1.if_nest > 0 {
                let idx = p1.if_nest as usize;
                set_if_matched(&mut p1.if_matched, idx, true);
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Elseif => {
            if p1.if_nest > 0 {
                let idx = p1.if_nest as usize;
                set_if_matched(&mut p1.if_matched, idx, true);
                p1.is_skip = true;
                p1.skip_nest = p1.if_nest;
            }
        }
        InsnHandler::Endif => {
            if p1.if_nest > 0 {
                let idx = p1.if_nest as usize;
                set_if_matched(&mut p1.if_matched, idx, false);
                p1.if_nest -= 1;
            }
        }
        _ => {}
    }
}



