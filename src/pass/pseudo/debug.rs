//! Pseudo-instruction handlers for SCD debugging
//!
//! Handles: SCD (Source Code Debugging) pseudo-instructions
//! Provides debug information for linked objects.

/// SCD record entry type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScdRecordType {
    /// Source line mapping
    SourceLine = 0x01,
    /// Symbol definition
    SymbolDef = 0x02,
    /// Symbol reference
    SymbolRef = 0x03,
}

/// Helper to parse SCD file argument
pub fn parse_scd_filename(line: &[u8], pos: &mut usize) -> Vec<u8> {
    while *pos < line.len() && line[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    let start = *pos;
    while *pos < line.len() && !line[*pos].is_ascii_whitespace() {
        *pos += 1;
    }
    line[start..*pos].to_vec()
}

/// Validate SCD record type value
pub fn is_valid_scd_record_type(value: u8) -> bool {
    matches!(value, 0x01 | 0x02 | 0x03)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scd_record_type_variants() {
        assert_eq!(ScdRecordType::SourceLine as u8, 0x01);
        assert_eq!(ScdRecordType::SymbolDef as u8, 0x02);
        assert_eq!(ScdRecordType::SymbolRef as u8, 0x03);
    }

    #[test]
    fn test_parse_scd_filename_simple() {
        let line = b"prog.s";
        let mut pos = 0;
        let fname = parse_scd_filename(line, &mut pos);
        assert_eq!(fname, b"prog.s");
        assert_eq!(pos, 6);
    }

    #[test]
    fn test_parse_scd_filename_with_spaces() {
        let line = b"  debug.scd  more";
        let mut pos = 0;
        let fname = parse_scd_filename(line, &mut pos);
        assert_eq!(fname, b"debug.scd");
    }

    #[test]
    fn test_is_valid_scd_record_type() {
        assert!(is_valid_scd_record_type(0x01));
        assert!(is_valid_scd_record_type(0x02));
        assert!(is_valid_scd_record_type(0x03));
        assert!(!is_valid_scd_record_type(0x00));
        assert!(!is_valid_scd_record_type(0x04));
        assert!(!is_valid_scd_record_type(0xFF));
    }
}
