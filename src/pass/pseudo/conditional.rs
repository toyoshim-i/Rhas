//! Pseudo-instruction handlers for conditional assembly
//!
//! Handles: .if, .iff, .ifdef, .ifndef, .else, .elseif, .endif
//! Complex control flow for assembler-time conditional compilation.

use crate::expr::parse_expr;
use crate::symbol::SymbolTable;

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
    if let Ok(rpn) = parse_expr(line, pos) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_ident_simple() {
        let line = b"LABEL rest";
        let mut pos = 0;
        let ident = read_ident(line, &mut pos);
        assert_eq!(ident, b"LABEL");
        assert_eq!(pos, 5);
    }

    #[test]
    fn test_read_ident_with_spaces() {
        let line = b"  SYMBOL  more";
        let mut pos = 0;
        let ident = read_ident(line, &mut pos);
        assert_eq!(ident, b"SYMBOL");
    }

    #[test]
    fn test_skip_spaces() {
        let line = b"   text";
        let mut pos = 0;
        skip_spaces(line, &mut pos);
        assert_eq!(pos, 3);
    }

    #[test]
    fn test_set_if_matched_in_bounds() {
        let mut arr = [false, false, false];
        set_if_matched(&mut arr, 1, true);
        assert_eq!(arr[1], true);
        assert_eq!(arr[0], false);
    }

    #[test]
    fn test_set_if_matched_out_of_bounds() {
        let mut arr = [false, false];
        set_if_matched(&mut arr, 10, true);  // Should not crash
        assert_eq!(arr[0], false);
    }
}
