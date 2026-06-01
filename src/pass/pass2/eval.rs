use crate::addressing::{Displacement, EffectiveAddress};
use crate::expr::eval::EvalValue;
use crate::expr::{eval_rpn, RPNToken, Rpn};
use crate::instructions::{encode_insn, InsnError};
use crate::symbol::types::{DefAttrib, InsnHandler, SizeCode};
use crate::symbol::{Symbol, SymbolTable};

pub(super) fn estimate_deferred_size(
    sym: &SymbolTable,
    base: u16,
    handler: InsnHandler,
    size: SizeCode,
    ops: &[EffectiveAddress],
    loc: u32,
    sect: u8,
) -> u32 {
    // DBcc は常に opcode + disp16 の 4 バイト。
    if matches!(handler, InsnHandler::DBcc) {
        return 4;
    }
    if matches!(handler, InsnHandler::FDBcc) {
        return 6;
    }
    if matches!(handler, InsnHandler::FBcc) {
        return match size {
            SizeCode::Long => 6,
            SizeCode::Word => 4,
            SizeCode::None => {
                // 自動サイズ: まず .w を試し、収まらなければ .l
                let target = match ops.first() {
                    Some(EffectiveAddress::AbsLong(rpn))
                    | Some(EffectiveAddress::AbsShort(rpn)) => {
                        eval_rpn_with_sym(sym, rpn, loc, sect).map(|ev| ev.value)
                    }
                    _ => None,
                };
                if let Some(target_addr) = target {
                    let disp = target_addr - (loc as i32 + 2);
                    if (-32768..=32767).contains(&disp) {
                        4
                    } else {
                        6
                    }
                } else {
                    4
                }
            }
            _ => 4,
        };
    }

    let resolved_ops: Vec<EffectiveAddress> = ops
        .iter()
        .map(|ea| resolve_ea_for_pass2(sym, ea, loc, sect))
        .collect();

    match encode_insn(base, handler, size, &resolved_ops) {
        Ok(bytes) => bytes.len() as u32,
        Err(InsnError::DeferToLinker) => {
            2 + resolved_ops
                .iter()
                .map(|ea| ea_ext_size_for_insn(ea, size))
                .sum::<u32>()
        }
        Err(_) => {
            2 + resolved_ops
                .iter()
                .map(|ea| ea_ext_size_for_insn(ea, size))
                .sum::<u32>()
        }
    }
}

fn resolve_ea_for_pass2(
    sym: &SymbolTable,
    ea: &EffectiveAddress,
    loc: u32,
    sect: u8,
) -> EffectiveAddress {
    let zero_rpn = || vec![RPNToken::Value(0), RPNToken::End];
    let one_rpn = || vec![RPNToken::Value(1), RPNToken::End];
    match ea {
        EffectiveAddress::Immediate(rpn) => {
            if let Some(ev) = eval_rpn_with_sym(sym, rpn, loc, sect) {
                EffectiveAddress::Immediate(vec![RPNToken::Value(ev.value as u32), RPNToken::End])
            } else {
                EffectiveAddress::Immediate(zero_rpn())
            }
        }
        EffectiveAddress::AbsShort(rpn) => {
            if let Some(ev) = eval_rpn_with_sym(sym, rpn, loc, sect) {
                EffectiveAddress::AbsShort(vec![RPNToken::Value(ev.value as u32), RPNToken::End])
            } else {
                EffectiveAddress::AbsShort(zero_rpn())
            }
        }
        EffectiveAddress::AbsLong(rpn) => {
            if let Some(ev) = eval_rpn_with_sym(sym, rpn, loc, sect) {
                EffectiveAddress::AbsLong(vec![RPNToken::Value(ev.value as u32), RPNToken::End])
            } else {
                EffectiveAddress::AbsLong(zero_rpn())
            }
        }
        EffectiveAddress::AddrRegDisp { an, disp } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                ea.clone()
            } else if let Some(ev) = eval_rpn_with_sym(sym, &disp.rpn, loc, sect) {
                EffectiveAddress::AddrRegDisp {
                    an: *an,
                    disp: Displacement {
                        rpn: vec![RPNToken::Value(ev.value as u32), RPNToken::End],
                        size: disp.size,
                        const_val: Some(ev.value),
                    },
                }
            } else {
                // 未解決時は 0 を使うと (An) へ短縮されすぎるので 1 を使う。
                EffectiveAddress::AddrRegDisp {
                    an: *an,
                    disp: Displacement {
                        rpn: one_rpn(),
                        size: disp.size,
                        const_val: Some(1),
                    },
                }
            }
        }
        EffectiveAddress::AddrRegIdx { an, disp, idx } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                ea.clone()
            } else if let Some(ev) = eval_rpn_with_sym(sym, &disp.rpn, loc, sect) {
                EffectiveAddress::AddrRegIdx {
                    an: *an,
                    disp: Displacement {
                        rpn: vec![RPNToken::Value(ev.value as u32), RPNToken::End],
                        size: disp.size,
                        const_val: Some(ev.value),
                    },
                    idx: idx.clone(),
                }
            } else {
                EffectiveAddress::AddrRegIdx {
                    an: *an,
                    disp: Displacement {
                        rpn: one_rpn(),
                        size: disp.size,
                        const_val: Some(1),
                    },
                    idx: idx.clone(),
                }
            }
        }
        EffectiveAddress::PcDisp(disp) => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                ea.clone()
            } else if let Some(ev) = eval_rpn_with_sym(sym, &disp.rpn, loc, sect) {
                let displacement = ev.value - (loc as i32 + 2);
                EffectiveAddress::PcDisp(Displacement {
                    rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                    size: disp.size,
                    const_val: Some(displacement),
                })
            } else {
                EffectiveAddress::PcDisp(Displacement {
                    rpn: zero_rpn(),
                    size: disp.size,
                    const_val: Some(0),
                })
            }
        }
        EffectiveAddress::PcIdx { disp, idx } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                ea.clone()
            } else if let Some(ev) = eval_rpn_with_sym(sym, &disp.rpn, loc, sect) {
                let displacement = ev.value - (loc as i32 + 2);
                EffectiveAddress::PcIdx {
                    disp: Displacement {
                        rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                        size: disp.size,
                        const_val: Some(displacement),
                    },
                    idx: idx.clone(),
                }
            } else {
                EffectiveAddress::PcIdx {
                    disp: Displacement {
                        rpn: zero_rpn(),
                        size: disp.size,
                        const_val: Some(0),
                    },
                    idx: idx.clone(),
                }
            }
        }
        EffectiveAddress::MemIndPost { an, bd, idx, od } => {
            let new_bd = resolve_disp_for_pass2(sym, bd, loc, sect, false);
            let new_od = resolve_disp_for_pass2(sym, od, loc, sect, false);
            EffectiveAddress::MemIndPost {
                an: *an,
                bd: new_bd,
                idx: idx.clone(),
                od: new_od,
            }
        }
        EffectiveAddress::MemIndPre { an, bd, idx, od } => {
            let new_bd = resolve_disp_for_pass2(sym, bd, loc, sect, false);
            let new_od = resolve_disp_for_pass2(sym, od, loc, sect, false);
            EffectiveAddress::MemIndPre {
                an: *an,
                bd: new_bd,
                idx: idx.clone(),
                od: new_od,
            }
        }
        EffectiveAddress::PcMemIndPost { bd, idx, od } => {
            let new_bd = resolve_disp_for_pass2(sym, bd, loc, sect, true);
            let new_od = resolve_disp_for_pass2(sym, od, loc, sect, false);
            EffectiveAddress::PcMemIndPost {
                bd: new_bd,
                idx: idx.clone(),
                od: new_od,
            }
        }
        EffectiveAddress::PcMemIndPre { bd, idx, od } => {
            let new_bd = resolve_disp_for_pass2(sym, bd, loc, sect, true);
            let new_od = resolve_disp_for_pass2(sym, od, loc, sect, false);
            EffectiveAddress::PcMemIndPre {
                bd: new_bd,
                idx: idx.clone(),
                od: new_od,
            }
        }
        _ => ea.clone(),
    }
}

fn resolve_disp_for_pass2(
    sym: &SymbolTable,
    d: &Displacement,
    loc: u32,
    sect: u8,
    is_pc_rel: bool,
) -> Displacement {
    if d.const_val.is_some() || d.rpn.is_empty() {
        return d.clone();
    }
    if let Some(ev) = eval_rpn_with_sym(sym, &d.rpn, loc, sect) {
        let val = if is_pc_rel {
            ev.value - (loc as i32 + 2)
        } else {
            ev.value
        };
        Displacement {
            rpn: vec![RPNToken::Value(val as u32), RPNToken::End],
            size: d.size,
            const_val: Some(val),
        }
    } else {
        // 未解決: 非ゼロプレースホルダー（BD=0 だと null displacement 扱いになるため）
        Displacement {
            rpn: vec![RPNToken::Value(1), RPNToken::End],
            size: d.size,
            const_val: Some(1),
        }
    }
}

pub(super) fn eval_rpn_with_sym(
    sym: &SymbolTable,
    rpn: &Rpn,
    loc: u32,
    sect: u8,
) -> Option<EvalValue> {
    eval_rpn(rpn, loc, loc, sect, &|name| {
        sym.lookup_sym(name).and_then(|s| {
            if let Symbol::Value {
                value,
                section,
                attrib,
                ..
            } = s
            {
                if *attrib >= DefAttrib::NoDet {
                    return Some(EvalValue {
                        value: *value,
                        section: *section,
                    });
                }
            }
            None
        })
    })
    .ok()
}

fn ea_ext_size_for_insn(ea: &EffectiveAddress, size: SizeCode) -> u32 {
    match ea {
        EffectiveAddress::Immediate(_) => match size {
            SizeCode::Long => 4,
            _ => 2,
        },
        other => ea_ext_size(other),
    }
}

fn ea_ext_size(ea: &EffectiveAddress) -> u32 {
    match ea {
        EffectiveAddress::DataReg(_)
        | EffectiveAddress::AddrReg(_)
        | EffectiveAddress::AddrRegInd(_)
        | EffectiveAddress::AddrRegPostInc(_)
        | EffectiveAddress::AddrRegPreDec(_) => 0,
        EffectiveAddress::AbsShort(_)
        | EffectiveAddress::AddrRegDisp { .. }
        | EffectiveAddress::PcDisp(_) => 2,
        EffectiveAddress::AbsLong(_) => 4,
        EffectiveAddress::Immediate(_) => 2,
        EffectiveAddress::AddrRegIdx { .. } | EffectiveAddress::PcIdx { .. } => 2,
        EffectiveAddress::MemIndPost { .. }
        | EffectiveAddress::MemIndPre { .. }
        | EffectiveAddress::PcMemIndPost { .. }
        | EffectiveAddress::PcMemIndPre { .. } => 6,
        EffectiveAddress::CcrReg
        | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_)
        | EffectiveAddress::FpCtrlReg(_) => 0,
    }
}
