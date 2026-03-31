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
    use crate::expr::rpn::RPNToken;
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
            p1.ctx.scd_temp = crate::context::ScdTemp::default();
            p1.ctx.scd_temp.name = name.clone();
            if let Some(attr) = scd_special_attr(&name) {
                p1.ctx.scd_temp.attrib = attr;
            }
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
        }
        crate::symbol::types::InsnHandler::Endef => {
            let t = p1.ctx.scd_temp.clone();
            records.push(crate::pass::temp::TempRecord::ScdEndef {
                name: t.name,
                attrib: t.attrib,
                value: t.value,
                section: t.section,
                scl: t.scl,
                type_code: t.type_code,
                size: t.size,
                dim: t.dim,
                is_long: t.is_long,
            });
            p1.ctx.scd_temp = crate::context::ScdTemp::default();
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
        }
        crate::symbol::types::InsnHandler::Val => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    p1.ctx.scd_temp.value = v.value as u32;
                    p1.ctx.scd_temp.section = if v.section == 0 { -1 } else { v.section as i16 };
                }
                records.push(crate::pass::temp::TempRecord::ScdVal { rpn });
            } else {
                p1.error_code(ErrorCode::Expr, None);
            }
        }
        crate::symbol::types::InsnHandler::Scl => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let scl = v.value as i32;
                    if scl == -1 {
                        p1.ctx.scd_temp.scl = 0xFF;
                        records.push(crate::pass::temp::TempRecord::ScdFuncEnd {
                            location: p1.location(),
                            section: p1.section_id(),
                        });
                    } else if (0..=255).contains(&scl) {
                        p1.ctx.scd_temp.scl = scl as u8;
                    } else {
                        p1.error_code(ErrorCode::IlValue, None);
                    }
                }
            } else {
                p1.error_code(ErrorCode::Expr, None);
            }
        }
        crate::symbol::types::InsnHandler::TypeScd => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    p1.ctx.scd_temp.type_code = v.value as u16;
                }
            }
        }
        crate::symbol::types::InsnHandler::Tag => {
            skip_spaces(line, pos);
            let tag = read_ident(line, pos);
            if !tag.is_empty() {
                records.push(crate::pass::temp::TempRecord::ScdTag { name: tag });
            }
        }
        crate::symbol::types::InsnHandler::Ln => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let line_no = v.value as u16;
                    skip_spaces(line, pos);
                    let loc = if *pos < line.len() && line[*pos] == b',' {
                        *pos += 1;
                        skip_spaces(line, pos);
                        parse_expr(line, pos).unwrap_or_else(|_| vec![RPNToken::Location, RPNToken::End])
                    } else {
                        vec![RPNToken::Location, RPNToken::End]
                    };
                    records.push(crate::pass::temp::TempRecord::ScdLn { line: line_no, loc });
                }
            }
        }
        crate::symbol::types::InsnHandler::Line => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let line_no = v.value as u16;
                    records.push(crate::pass::temp::TempRecord::ScdLn {
                        line: line_no,
                        loc: vec![RPNToken::Location, RPNToken::End],
                    });
                }
            }
        }
        crate::symbol::types::InsnHandler::SizeScd => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    p1.ctx.scd_temp.size = v.value as u32;
                }
            }
        }
        crate::symbol::types::InsnHandler::Dim => {
            skip_spaces(line, pos);
            let mut dim_idx = 0usize;
            while *pos < line.len() && dim_idx < 4 {
                if let Ok(rpn) = parse_expr(line, pos) {
                    if let Some(v) = p1.eval_const(&rpn) {
                        p1.ctx.scd_temp.dim[dim_idx] = v.value as u16;
                    }
                    dim_idx += 1;
                    skip_spaces(line, pos);
                    if *pos < line.len() && line[*pos] == b',' {
                        *pos += 1;
                        skip_spaces(line, pos);
                        continue;
                    }
                }
                break;
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
