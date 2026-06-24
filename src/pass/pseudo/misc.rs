//! Pseudo-instruction handlers for miscellaneous directives
//!
//! Handles: .org, .fail, .cpu, .globl, .extern, .comm, .even, .align, etc.
//! These are less complex directives not covered by other modules.

// HAS060互換の各種疑似命令ハンドラ（アラインメントやCPUディレクティブ）や、将来用いる予定の
// 列挙型バリアント（CpuDirective等）が、現状のビルドで一部未参照となっている警告を抑制するために付与しています。
#![allow(dead_code)]

/// CPU type specification support
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuDirective {
    /// .cpu 68000
    Cpu68000 = 68000,
    /// .cpu 68010
    Cpu68010 = 68010,
    /// .cpu 68020
    Cpu68020 = 68020,
    /// .cpu 68030
    Cpu68030 = 68030,
    /// .cpu 68040
    Cpu68040 = 68040,
    /// .cpu 68060
    Cpu68060 = 68060,
}

impl CpuDirective {
    /// Parse CPU number from input
    pub fn from_number(n: u32) -> Option<Self> {
        match n {
            68000 => Some(CpuDirective::Cpu68000),
            68010 => Some(CpuDirective::Cpu68010),
            68020 => Some(CpuDirective::Cpu68020),
            68030 => Some(CpuDirective::Cpu68030),
            68040 => Some(CpuDirective::Cpu68040),
            68060 => Some(CpuDirective::Cpu68060),
            _ => None,
        }
    }

    /// Get CPU number value
    pub fn number(&self) -> u32 {
        *self as u32
    }

    /// Check if CPU supports instruction
    pub fn supports_fpu(&self) -> bool {
        self.number() >= 68040
    }
}

/// Alignment specifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentOperand {
    /// 2-byte alignment (.even)
    Even = 2,
    /// 4-byte alignment
    Quad = 4,
    /// 8-byte alignment
    Octa = 8,
    /// 16-byte alignment
    Hex = 16,
}

impl AlignmentOperand {
    /// Get alignment boundary size
    pub fn boundary(&self) -> u32 {
        *self as u32
    }
}

/// Helper to parse .org argument
pub fn parse_org_address(value: u32) -> u32 {
    value
}


// bring in utilities needed by handler
use crate::expr::parse_expr;
use crate::error::ErrorCode;
use crate::options::cpu as cpuconst;
use crate::symbol::Symbol;
use crate::symbol::types::{DefAttrib, ExtAttrib, FirstDef, InsnHandler};
use super::super::temp::TempRecord;
use crate::pass::pass1::{skip_spaces, read_ident, parse_align_n, parse_align_pad};

/// Central dispatcher for miscellaneous pseudo-instructions.
/// The goal is to keep pass1.rs lean by moving these handlers here.
pub fn handle_misc(
    handler: InsnHandler,
    label: &Option<Vec<u8>>,
    line: &[u8],
    pos: &mut usize,
    p1: &mut crate::pass::pass1::P1Ctx<'_>,
    records: &mut Vec<TempRecord>,
) {
    match handler {
        InsnHandler::Fail => {
            // .fail <式> — 式が非0（または式なし）のときアセンブルエラー
            skip_spaces(line, pos);
            let should_fail = if *pos < line.len() {
                if let Ok(rpn) = parse_expr(line, pos) {
                    p1.eval_const(&rpn).map(|v| v.value != 0).unwrap_or(true)
                } else {
                    true
                }
            } else {
                true
            };
            if should_fail {
                p1.error_code(ErrorCode::Forced, None);
            }
        }
        InsnHandler::Globl => {
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::Globl { name: name.clone() });
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }
        InsnHandler::Xref => {
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::XRef { name: name.clone() });
                if p1.sym.lookup_sym(&name).is_none() {
                    let sym = Symbol::Value {
                        attrib:     DefAttrib::Undef,
                        ext_attrib: ExtAttrib::XRef,
                        section:    0xFF,
                        org_num:    0,
                        first:      FirstDef::Other,
                        opt_count:  0,
                        value:      0,
                    };
                    p1.sym.define(name, sym);
                }
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }
        InsnHandler::Xdef => {
            // label-as-xdef and operand list
            if let Some(ref name) = label {
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
            }
            skip_spaces(line, pos);
            while *pos < line.len() && line[*pos] != b';' {
                let name = read_ident(line, pos);
                if name.is_empty() { break; }
                records.push(TempRecord::XDef { name: name.clone() });
                if let Some(Symbol::Value { ext_attrib, .. }) = p1.sym.lookup_sym_mut(&name) {
                    *ext_attrib = ExtAttrib::XDef;
                }
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else { break; }
            }
        }
        InsnHandler::Offset => {
            // .offset / .org
            skip_spaces(line, pos);
            let val = if *pos < line.len() {
                if let Ok(rpn) = parse_expr(line, pos) {
                    p1.eval_const(&rpn).map(|v| v.value as u32).unwrap_or(0)
                } else { 0 }
            } else { 0 };
            p1.ctx.offsym_with_symbol = false;
            p1.ctx.set_offset_mode(val);
        }
        InsnHandler::Cpu => {
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let num = v.value as u32;
                    if let Some(cpu) = crate::options::cpu_number_to_type(num) {
                        p1.ctx.set_cpu(cpu);
                        records.push(TempRecord::Cpu { cpu });
                    } else {
                        p1.error_code(ErrorCode::FeatureCpu, None);
                    }
                } else {
                    p1.error_code(ErrorCode::Expr, None);
                }
            } else {
                p1.error_code(ErrorCode::Expr, None);
            }
        }
        InsnHandler::Cpu68000 => {
            let cpu = crate::context::CpuType::new(68000, cpuconst::C000);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu68010 => {
            let cpu = crate::context::CpuType::new(68010, cpuconst::C010);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu68020 => {
            let cpu = crate::context::CpuType::new(68020, cpuconst::C020);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu68030 => {
            let cpu = crate::context::CpuType::new(68030, cpuconst::C030);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu68040 => {
            let cpu = crate::context::CpuType::new(68040, cpuconst::C040);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu68060 => {
            let cpu = crate::context::CpuType::new(68060, cpuconst::C060);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu5200 => {
            let cpu = crate::context::CpuType::new(5200, cpuconst::C520);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu5300 => {
            let cpu = crate::context::CpuType::new(5300, cpuconst::C530);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Cpu5400 => {
            let cpu = crate::context::CpuType::new(5400, cpuconst::C540);
            p1.ctx.set_cpu(cpu);
            records.push(TempRecord::Cpu { cpu });
        }
        InsnHandler::Even => {
            if p1.ctx.offsym_with_symbol {
                p1.error_code(ErrorCode::OffsymAlign, Some(b".even"));
                return;
            }
            if p1.is_offset_mode() {
                let loc = p1.location();
                if !loc.is_multiple_of(2) { p1.advance(1); }
            } else {
                let sec = p1.section_id();
                let pad = if sec == 0x01 { 0x4E71u16 } else { 0u16 };
                records.push(TempRecord::Align { n: 1, pad, section: sec });
            }
        }
        InsnHandler::Quad => {
            if p1.ctx.offsym_with_symbol {
                p1.error_code(ErrorCode::OffsymAlign, Some(b".quad"));
                return;
            }
            if p1.is_offset_mode() {
                let loc = p1.location();
                let mask = 4u32 - 1;
                if loc & mask != 0 { p1.advance(4 - (loc & mask)); }
            } else {
                let sec = p1.section_id();
                if 2 > p1.ctx.max_align { p1.ctx.max_align = 2; }
                records.push(TempRecord::Align { n: 2, pad: 0, section: sec });
            }
        }
        InsnHandler::Align => {
            if p1.ctx.offsym_with_symbol {
                p1.error_code(ErrorCode::OffsymAlign, Some(b".align"));
                return;
            }
            skip_spaces(line, pos);
            if let Some(n) = parse_align_n(line, pos, p1) {
                if p1.is_offset_mode() {
                    let align = 1u32 << n;
                    let loc = p1.location();
                    if !loc.is_multiple_of(align) { p1.advance(align - (loc % align)); }
                } else {
                    let sec = p1.section_id();
                    let pad = parse_align_pad(line, pos, p1).unwrap_or({
                        if sec == 0x01 { 0x4E71 } else { 0 }
                    });
                    if n > p1.ctx.max_align {
                        p1.ctx.max_align = n;
                    }
                    records.push(TempRecord::Align { n, pad, section: sec });
                }
            }
        }
        InsnHandler::Comm | InsnHandler::Rcomm | InsnHandler::Rlcomm => {
            let ext = match handler {
                InsnHandler::Comm => ExtAttrib::Comm,
                InsnHandler::Rcomm => ExtAttrib::RComm,
                InsnHandler::Rlcomm => ExtAttrib::RLComm,
                _ => ExtAttrib::Comm,
            };
            skip_spaces(line, pos);
            let name = read_ident(line, pos);
            if name.is_empty() {
                p1.error_code(ErrorCode::NoSymPseudo, Some(b".comm"));
                return;
            }
            skip_spaces(line, pos);
            if *pos >= line.len() || line[*pos] != b',' {
                p1.error_code(ErrorCode::IlOpr, None);
                return;
            }
            *pos += 1;
            skip_spaces(line, pos);

            let value = match parse_expr(line, pos).ok().and_then(|rpn| p1.eval_const(&rpn)) {
                Some(v) => v.value,
                None => {
                    p1.error_code(ErrorCode::Expr, None);
                    0
                }
            };
            if value <= 0 {
                p1.error_code(ErrorCode::IlValue, None);
                return;
            }
            let sym = Symbol::Value {
                attrib: DefAttrib::Define,
                ext_attrib: ext,
                section:    0,
                org_num:    0,
                first:      FirstDef::Other,
                opt_count:  0,
                value,
            };
            p1.sym.define(name.clone(), sym);
            records.push(TempRecord::Comm { name, ext });
        }
        _ => {}
    }
}



