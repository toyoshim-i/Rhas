use crate::addressing::parse_reg_list_mask;
use crate::error::{warn, ErrorCode};
use crate::expr::rpn::RPNToken;
use crate::expr::{parse_expr, Rpn};
use crate::options::cpu as cpuconst;
use crate::pass::pseudo;
use crate::pass::temp::TempRecord;
use crate::source::SourceStack;
use crate::symbol::types::{DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeCode};
use crate::symbol::{Symbol, SymbolTable};

use super::{skip_spaces, P1Ctx};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_pseudo(
    handler: InsnHandler,
    mnem: &[u8],
    size: Option<SizeCode>,
    line: &[u8],
    pos: &mut usize,
    label: &Option<Vec<u8>>,
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
    source: &mut SourceStack,
) {
    let _ = mnem;
    match handler {
        // ---- セクション切り替え ----
        InsnHandler::TextSect
        | InsnHandler::DataSect
        | InsnHandler::BssSect
        | InsnHandler::Stack
        | InsnHandler::RdataSect
        | InsnHandler::RbssSect
        | InsnHandler::RstackSect
        | InsnHandler::RldataSect
        | InsnHandler::RlbssSect
        | InsnHandler::RlstackSect => {
            pseudo::section::handle_section(handler, p1.ctx, records);
        }

        // ---- .offset / .even / .quad / .align ----
        InsnHandler::Offset | InsnHandler::Even | InsnHandler::Quad | InsnHandler::Align => {
            pseudo::misc::handle_misc(handler, label, line, pos, p1, records);
        }

        // ---- .dc/.ds/.dcb ----
        InsnHandler::Dc | InsnHandler::Ds | InsnHandler::Dcb => {
            pseudo::data::handle_data(handler, size, line, pos, p1, records, source);
        }

        // ---- .equ / .set ----
        InsnHandler::Equ | InsnHandler::Set => {
            skip_spaces(line, pos);
            if let Some(ref name) = label {
                if let Ok(rpn) = parse_expr(line, pos) {
                    records.push(TempRecord::EquDef {
                        name: name.clone(),
                        rpn: rpn.clone(),
                    });
                    if let Some(v) = p1.eval_const(&rpn) {
                        let attrib = match handler {
                            // .set は時系列値としてその時点で確定させる
                            InsnHandler::Set => DefAttrib::Define,
                            // .equ はロケーション依存式なら後段で再評価
                            _ => {
                                if is_dynamic_equ_expr(&rpn, p1.sym) {
                                    DefAttrib::NoDet
                                } else {
                                    DefAttrib::Define
                                }
                            }
                        };
                        let sym = Symbol::Value {
                            attrib,
                            ext_attrib: ExtAttrib::None,
                            section: v.section,
                            org_num: 0,
                            first: FirstDef::Other,
                            opt_count: 0,
                            value: v.value,
                        };
                        p1.sym.define(name.clone(), sym);
                    } else {
                        let sym = Symbol::Value {
                            attrib: DefAttrib::NoDet,
                            ext_attrib: ExtAttrib::None,
                            section: 0,
                            org_num: 0,
                            first: FirstDef::Other,
                            opt_count: 0,
                            value: 0,
                        };
                        p1.sym.define(name.clone(), sym);
                    }
                }
            }
        }

        // ---- .xdef ----
        InsnHandler::Xdef => {
            if let Some(ref name) = label {
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
            }
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() {
                    break;
                }
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(&name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else {
                    break;
                }
            }
        }

        // ---- .xref ----
        InsnHandler::Xref => {
            pseudo::misc::handle_misc(handler, label, line, pos, p1, records);
        }

        // ---- .if / .ifdef / .ifndef / .else / .elseif / .endif ----
        InsnHandler::If
        | InsnHandler::Iff
        | InsnHandler::Ifdef
        | InsnHandler::Ifndef
        | InsnHandler::Else
        | InsnHandler::Elseif
        | InsnHandler::Endif => {
            pseudo::conditional::handle_conditional(handler, line, pos, p1);
        }

        // ---- .include ----
        InsnHandler::Include | InsnHandler::Insert => {
            skip_spaces(line, pos);
            let fname = parse_filename(line, pos);
            if !fname.is_empty() {
                let _ = source.push_include(&fname);
            }
        }

        // ---- .request ----
        InsnHandler::Request => {
            skip_spaces(line, pos);
            let fname = parse_filename(line, pos);
            if !fname.is_empty() {
                p1.ctx.request_files.push(fname);
            }
        }

        // ---- .end ----
        InsnHandler::End => {
            records.push(TempRecord::End);
            p1.is_end = true;
        }

        // ---- .cpu / CPU 指定 ----
        InsnHandler::Cpu
        | InsnHandler::Cpu68000
        | InsnHandler::Cpu68010
        | InsnHandler::Cpu68020
        | InsnHandler::Cpu68030
        | InsnHandler::Cpu68040
        | InsnHandler::Cpu68060
        | InsnHandler::Cpu5200
        | InsnHandler::Cpu5300
        | InsnHandler::Cpu5400 => {
            pseudo::misc::handle_misc(handler, label, line, pos, p1, records);
        }

        // ---- リスト制御 ----
        InsnHandler::List => {
            p1.ctx.prn_listing = true;
        }
        InsnHandler::Nlist => {
            p1.ctx.prn_listing = false;
        }
        InsnHandler::Lall => {
            p1.ctx.prn_macro_listing = true;
        }
        InsnHandler::Sall => {
            p1.ctx.prn_macro_listing = false;
        }
        InsnHandler::Width => {
            skip_spaces(line, pos);
            match parse_expr(line, pos)
                .ok()
                .and_then(|rpn| p1.eval_const(&rpn).map(|v| v.value))
            {
                Some(v) if (80..=255).contains(&v) => {
                    p1.ctx.opts.prn_width = (v as u16) & !7;
                }
                _ => {
                    p1.error_code(ErrorCode::IlValue, None);
                }
            }
        }
        InsnHandler::Page => {
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] == b';' {
                // 改ページのみ（値変更なし）
            } else if line[*pos] == b'+' {
                // `.page +`（値変更なし）
            } else {
                match parse_expr(line, pos)
                    .ok()
                    .and_then(|rpn| p1.eval_const(&rpn).map(|v| v.value))
                {
                    Some(v) if v < 0 => {
                        p1.ctx.opts.prn_page_lines = u16::MAX;
                    }
                    Some(v) if (10..=255).contains(&v) => {
                        p1.ctx.opts.prn_page_lines = v as u16;
                    }
                    _ => {
                        p1.error_code(ErrorCode::IlValue, None);
                    }
                }
            }
        }
        InsnHandler::Title => {
            p1.ctx.prn_title = parse_prn_text(line, pos);
        }
        InsnHandler::SubTtl => {
            p1.ctx.prn_subttl = parse_prn_text(line, pos);
        }

        // ---- .fail ----
        InsnHandler::Fail => {
            pseudo::misc::handle_misc(handler, label, line, pos, p1, records);
        }

        // ---- macro-style pseudos ----
        InsnHandler::MacroDef | InsnHandler::Rept | InsnHandler::Irp | InsnHandler::Irpc => {
            pseudo::macro_::handle_macro(handler, label.clone(), line, pos, source, p1, records);
        }

        // ---- .endm / .exitm / .local / .sizem ----
        InsnHandler::EndM | InsnHandler::ExitM | InsnHandler::Local | InsnHandler::SizeM => {}

        // ---- SCD デバッグ ----
        InsnHandler::FileScd
        | InsnHandler::Def
        | InsnHandler::Endef
        | InsnHandler::Val
        | InsnHandler::Scl
        | InsnHandler::TypeScd
        | InsnHandler::Tag
        | InsnHandler::Ln
        | InsnHandler::Line
        | InsnHandler::SizeScd
        | InsnHandler::Dim => {
            pseudo::debug::handle_scd(handler, line, pos, p1, records);
        }

        // ---- .reg ----
        InsnHandler::Reg => {
            skip_spaces(line, pos);
            if let Some(ref name) = label {
                let saved_pos = *pos;
                let reg_mask = parse_reg_list_mask(line, pos, p1.sym, p1.ctx.cpu.features);
                let rpns: Vec<Rpn> = if let Some(mask) = reg_mask {
                    vec![vec![RPNToken::ValueWord(mask), RPNToken::End]]
                } else {
                    *pos = saved_pos;
                    let mut list: Vec<Rpn> = Vec::new();
                    loop {
                        if *pos >= line.len() || line[*pos] == b';' {
                            break;
                        }
                        match parse_expr(line, pos) {
                            Ok(rpn) => list.push(rpn),
                            Err(_) => break,
                        }
                        skip_spaces(line, pos);
                        if *pos < line.len() && line[*pos] == b',' {
                            *pos += 1;
                            skip_spaces(line, pos);
                        } else {
                            break;
                        }
                    }
                    if list.len() == 1 {
                        if let [RPNToken::SymbolRef(target), RPNToken::End] = list[0].as_slice() {
                            let target = target.clone();
                            if p1.sym.lookup_sym(&target).is_none() {
                                let sym = Symbol::Value {
                                    attrib: DefAttrib::Undef,
                                    ext_attrib: ExtAttrib::XRef,
                                    section: 0xFF,
                                    org_num: 0,
                                    first: FirstDef::Other,
                                    opt_count: 0,
                                    value: 0,
                                };
                                p1.sym.define(target.clone(), sym);
                            }
                            records.push(TempRecord::XRef { name: target });
                        }
                    }
                    list
                };
                p1.sym.define(name.clone(), Symbol::RegSym { define: rpns });
            }
        }

        // ---- .comm / .rcomm / .rlcomm ----
        InsnHandler::Comm | InsnHandler::Rcomm | InsnHandler::Rlcomm => {
            pseudo::misc::handle_misc(handler, label, line, pos, p1, records);
        }

        // ---- .offsym ----
        InsnHandler::OffsymPs => {
            skip_spaces(line, pos);
            let init = if *pos < line.len() {
                if let Ok(rpn) = parse_expr(line, pos) {
                    p1.eval_const(&rpn).map(|v| v.value).unwrap_or(0)
                } else {
                    p1.error_code(ErrorCode::Expr, None);
                    return;
                }
            } else {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            };

            skip_spaces(line, pos);
            let mut has_symbol = false;
            if *pos < line.len() && line[*pos] == b',' {
                *pos += 1;
                skip_spaces(line, pos);
                let name = read_ident(line, pos);
                if name.is_empty() {
                    p1.error_code(ErrorCode::NoSymPseudo, Some(b".offsym"));
                    return;
                }
                let mut warn_overwrite = false;
                match p1.sym.lookup_sym_mut(&name) {
                    Some(Symbol::Value {
                        attrib,
                        section,
                        first,
                        value,
                        ext_attrib,
                        ..
                    }) => {
                        if *first != FirstDef::Offsym && *attrib >= DefAttrib::Define {
                            if p1.ctx.opts.ow_offsym {
                                p1.error_code(ErrorCode::RedefOffsym, Some(&name));
                                return;
                            }
                            warn_overwrite = true;
                        }
                        *attrib = DefAttrib::Define;
                        *ext_attrib = ExtAttrib::None;
                        *section = 0;
                        *first = FirstDef::Offsym;
                        *value = init;
                    }
                    Some(_) => {
                        p1.error_code(ErrorCode::IlSymValue, None);
                        return;
                    }
                    None => {
                        let sym = Symbol::Value {
                            attrib: DefAttrib::Define,
                            ext_attrib: ExtAttrib::None,
                            section: 0,
                            org_num: 0,
                            first: FirstDef::Offsym,
                            opt_count: 0,
                            value: init,
                        };
                        p1.sym.define(name.clone(), sym);
                    }
                }
                if warn_overwrite {
                    p1.warn_code(warn::REDEF_OFFSYM, Some(&name));
                }
                has_symbol = true;
                skip_spaces(line, pos);
            }

            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            p1.ctx.offsym_with_symbol = has_symbol;
            p1.ctx.set_offset_mode(init as u32);
        }

        // ---- FP 等 ----
        InsnHandler::FpId => {
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] == b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            let value = match parse_expr(line, pos)
                .ok()
                .and_then(|rpn| p1.eval_const(&rpn))
            {
                Some(v) if v.section == 0 => v.value,
                _ => {
                    p1.error_code(ErrorCode::Expr, None);
                    return;
                }
            };
            skip_spaces(line, pos);
            if *pos < line.len() && line[*pos] != b';' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            if value < 0 {
                p1.ctx.cpu.features &= !cpuconst::CFPP;
            } else if value <= 7 {
                p1.ctx.fpid = value as u8;
            } else {
                p1.error_code(ErrorCode::IlValue, None);
            }
        }
        InsnHandler::Pragma => {}

        _ => {}
    }
}

fn is_dynamic_equ_expr(rpn: &Rpn, sym: &SymbolTable) -> bool {
    for tok in rpn {
        match tok {
            RPNToken::Location | RPNToken::CurrentLoc => return true,
            RPNToken::SymbolRef(name) => match sym.lookup_sym(name) {
                Some(Symbol::Value {
                    section, attrib, ..
                }) => {
                    if *attrib < DefAttrib::Define || *section != 0 {
                        return true;
                    }
                }
                _ => return true,
            },
            _ => {}
        }
    }
    false
}

pub(crate) fn parse_align_n(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u8> {
    if let Ok(rpn) = parse_expr(line, pos) {
        if let Some(v) = p1.eval_const(&rpn) {
            let align = v.value as u32;
            if align >= 2 {
                let mut n = 0u8;
                let mut a = align;
                while a > 1 {
                    a >>= 1;
                    n += 1;
                }
                return Some(n);
            }
        }
    }
    None
}

pub(crate) fn parse_align_pad(line: &[u8], pos: &mut usize, p1: &mut P1Ctx<'_>) -> Option<u16> {
    skip_spaces(line, pos);
    if *pos < line.len() && line[*pos] == b',' {
        *pos += 1;
        skip_spaces(line, pos);
        if let Ok(rpn) = parse_expr(line, pos) {
            if let Some(v) = p1.eval_const(&rpn) {
                return Some(v.value as u16);
            }
        }
    }
    None
}

pub(crate) fn read_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
    let start = *pos;
    while *pos < line.len() {
        let b = line[*pos];
        if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'@' {
            *pos += 1;
        } else {
            break;
        }
    }
    line[start..*pos].to_vec()
}

pub(crate) fn parse_string_or_ident(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() {
        return Vec::new();
    }
    if line[*pos] == b'"' || line[*pos] == b'\'' {
        let quote = line[*pos];
        *pos += 1;
        let start = *pos;
        while *pos < line.len() && line[*pos] != quote {
            *pos += 1;
        }
        let s = line[start..*pos].to_vec();
        if *pos < line.len() {
            *pos += 1;
        }
        s
    } else {
        read_ident(line, pos)
    }
}

pub(crate) fn parse_filename(line: &[u8], pos: &mut usize) -> Vec<u8> {
    if *pos >= line.len() {
        return Vec::new();
    }
    if line[*pos] == b'"' || line[*pos] == b'\'' {
        let quote = line[*pos];
        *pos += 1;
        let start = *pos;
        while *pos < line.len() && line[*pos] != quote {
            *pos += 1;
        }
        let s = line[start..*pos].to_vec();
        if *pos < line.len() {
            *pos += 1;
        }
        s
    } else {
        let start = *pos;
        while *pos < line.len() {
            let b = line[*pos];
            if b == b' ' || b == b'\t' || b == b';' {
                break;
            }
            *pos += 1;
        }
        line[start..*pos].to_vec()
    }
}

fn parse_prn_text(line: &[u8], pos: &mut usize) -> Vec<u8> {
    skip_spaces(line, pos);
    if *pos >= line.len() {
        return Vec::new();
    }

    let mut s = if line[*pos] == b'"' || line[*pos] == b'\'' {
        parse_string_or_ident(line, pos)
    } else {
        let start = *pos;
        while *pos < line.len() && line[*pos] != b';' {
            *pos += 1;
        }
        line[start..*pos].to_vec()
    };

    while let Some(&b) = s.last() {
        if b == b' ' || b == b'\t' {
            s.pop();
        } else {
            break;
        }
    }
    s
}
