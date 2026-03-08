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

/// Handle SCD debug pseudo-instruction from pass1
pub fn handle_scd(
    handler: crate::symbol::types::InsnHandler,
    line: &[u8],
    pos: &mut usize,
    p1: &mut crate::pass::pass1::P1Ctx<'_>,
    records: &mut Vec<crate::pass::temp::TempRecord>,
) {
    use crate::expr::parse_expr;
    use crate::error::ErrorCode;
    use crate::pass::pass1::{skip_spaces, read_ident, parse_filename};

    // HAS互換:
    // -g 指定時（MAKESYMDEB=true）は SCD 疑似命令を無視する。
    if p1.ctx.opts.make_sym_deb {
        return;
    }
    // HAS 互換: `.file` で SCD モード有効化されるまで、.file 以外は無視する。
    if handler != crate::symbol::types::InsnHandler::FileScd && !p1.ctx.scd_enabled {
        return;
    }
    match handler {
        crate::symbol::types::InsnHandler::FileScd => {
            skip_spaces(line, pos);
            let name = parse_filename(line, pos);
            if name.is_empty() {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            p1.ctx.scd_enabled = true;
            p1.ctx.scd_file = name;
        }
        crate::symbol::types::InsnHandler::Def => {
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            if name.is_empty() {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            p1.ctx.scd_sym = name;
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
        }
        crate::symbol::types::InsnHandler::Endef => {
            p1.ctx.scd_sym = Vec::new();
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
        }
        crate::symbol::types::InsnHandler::Val => {
            skip_spaces(line, pos);
            // <value> <operationspec>
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    skip_spaces(line, pos);
                    if *pos < line.len() && line[*pos] != b';' {
                        let op = read_ident(line, pos);
                        // TODO: op が指定されていない場合は .val の挙動どうなるのか
                        records.push(crate::pass::temp::TempRecord::ScdVal { value: v.value as u32, op });
                    }
                }
            }
        }
        crate::symbol::types::InsnHandler::Scl => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdScl { choice: v.value as u32 });
                }
            }
        }
        crate::symbol::types::InsnHandler::TypeScd => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdType { rec_type: v.value as u8 });
                }
            }
        }
        crate::symbol::types::InsnHandler::Tag => {
            skip_spaces(line, pos);
            let tag = read_ident(line, pos);
            if !tag.is_empty() {
                records.push(crate::pass::temp::TempRecord::ScdTag { tag });
            }
        }
        crate::symbol::types::InsnHandler::Ln => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdLn { line: v.value as u32 });
                }
            }
        }
        crate::symbol::types::InsnHandler::Line => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdLine { line: v.value as u32 });
                }
            }
        }
        crate::symbol::types::InsnHandler::SizeScd => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdSize { size: v.value as u32 });
                }
            }
        }
        crate::symbol::types::InsnHandler::Dim => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    records.push(crate::pass::temp::TempRecord::ScdDim { dim: v.value as u32 });
                }
            }
        }
        _ => {}
    }
}

/// Mirror of former pass1 helper for special SCD symbol attributes
pub(crate) fn scd_special_attr(name: &[u8]) -> Option<u8> {
    if name.eq_ignore_ascii_case(b".eos") {
        Some(0x1F)
    } else if name.eq_ignore_ascii_case(b".bb") {
        Some(0x2B)
    } else if name.eq_ignore_ascii_case(b".eb") {
        Some(0x2C)
    } else if name.eq_ignore_ascii_case(b".bf") {
        Some(0x2D)
    } else if name.eq_ignore_ascii_case(b".ef") {
        Some(0x2E)
    } else {
        None
    }
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
