//! Pseudo-instruction handlers for macro and repetition
//!
//! Handles: .macro, .endm, .rept, .irp, .irpc
//! Complex control flow for assembler-time macro expansion and repetitive code generation.

/// Helper to parse repetition count
pub fn parse_repeat_count(value: u32) -> u32 {
    if value == 0 { 0 } else { value }
}

/// Helper to track macro nesting depth
pub fn check_macro_nesting(current_depth: u16, max_depth: u16) -> bool {
    current_depth < max_depth
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
