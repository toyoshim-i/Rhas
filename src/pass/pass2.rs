/// Pass 2: 最適化（分岐サイズ縮小 + DeferredInsn サイズ再評価）
///
/// オリジナルの pass2 に対応。分岐命令のサイズ最適化に加えて、
/// Pass1 で未解決だった EA を再評価して命令長を更新する。
/// 収束するまで繰り返す。

use crate::addressing::{Displacement, EffectiveAddress};
use crate::expr::{eval_rpn, Rpn, RPNToken};
use crate::expr::eval::EvalValue;
use crate::instructions::{encode_insn, InsnError};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::DefAttrib;
use crate::symbol::types::{InsnHandler, SizeCode};
use super::temp::{branch_word_size, TempRecord};

/// Pass2: TempRecord 列のロケーションカウンタを再計算し、分岐サイズを最適化する
///
/// 1. ロケーションカウンタを再計算してシンボルを更新する
/// 2. 最適化できる分岐（.w → .s）を縮小する
/// 3. 収束するまで繰り返す
pub fn pass2(records: &mut Vec<TempRecord>, sym: &mut SymbolTable) {
    // 最大反復回数（通常は少ない繰り返しで収束する）
    for _ in 0..32 {
        let changed = pass2_one(records, sym);
        if !changed { break; }
    }
}

/// 1回のパス: ロケーションカウンタ再計算 + 分岐縮小
/// 変化があれば true を返す
fn pass2_one(records: &mut Vec<TempRecord>, sym: &mut SymbolTable) -> bool {
    let mut loc_ctr = [0u32; 10];
    let mut cur_sect = 0usize;
    let mut changed = false;

    for rec in records.iter_mut() {
        match rec {
            TempRecord::SectChange { id } => {
                let idx = (*id as usize).saturating_sub(1);
                if idx < 10 { cur_sect = idx; }
            }
            TempRecord::Align { n, section, .. } => {
                let idx = (*section as usize).saturating_sub(1).min(9);
                let align = 1u32 << *n;
                let loc = loc_ctr[idx];
                let new_loc = (loc + align - 1) & !(align - 1);
                loc_ctr[idx] = new_loc;
            }
            TempRecord::LabelDef { name, section, offset } => {
                if *section == 0 {
                    // ABS セクション（.offset モード）はロケーションカウンタを使わない
                } else {
                    let idx = (*section as usize).saturating_sub(1).min(9);
                    let new_offset = loc_ctr[idx];
                    *offset = new_offset;
                    update_symbol(sym, name, *section, new_offset as i32);
                }
            }
            TempRecord::EquDef { name, rpn } => {
                let loc = loc_ctr[cur_sect];
                let sect = cur_sect as u8 + 1;
                if let Some(ev) = eval_rpn_with_sym(sym, rpn, loc, sect) {
                    if let Some(Symbol::Value { value, section, .. }) = sym.lookup_sym(name) {
                        if *value != ev.value || *section != ev.section {
                            changed = true;
                        }
                    } else {
                        changed = true;
                    }
                    update_symbol(sym, name, ev.section, ev.value);
                }
            }
            TempRecord::Org { value } => {
                loc_ctr[cur_sect] = *value;
            }
            TempRecord::DeferredInsn { base, handler, size, ops, byte_size } => {
                let loc = loc_ctr[cur_sect];
                let sect = cur_sect as u8 + 1;
                let new_size = estimate_deferred_size(sym, *base, *handler, *size, ops, loc, sect);
                if *byte_size != new_size {
                    *byte_size = new_size;
                    changed = true;
                }
                loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(*byte_size);
            }
            TempRecord::Branch { opcode, req_size, cur_size, suppressed, target } => {
                let loc = loc_ctr[cur_sect];
                let mut new_cur_size = *cur_size;
                let mut new_suppressed = false;

                // サイズ未指定（自動）形式のみ最適化
                if req_size.is_none() {
                    if let Some(ev) = eval_target(sym, target, loc, cur_sect as u8 + 1) {
                        // 同一セクション参照のみ最適化対象
                        if ev.section as u8 == cur_sect as u8 + 1 {
                            let offset = (ev.value as i64) - (loc as i64 + 2);
                            let next_offset = if *suppressed {
                                -2
                            } else {
                                match cur_size {
                                Some(crate::symbol::types::SizeCode::Short) => 0,
                                Some(crate::symbol::types::SizeCode::Long) => 4,
                                _ => 2, // None=.w
                                }
                            };
                            // 直後への bra/bcc は命令自体を削除（bsr は除外）
                            if offset == next_offset && !is_bsr(*opcode) {
                                new_suppressed = true;
                                new_cur_size = None;
                            } else if *suppressed {
                                // HAS互換: 一度サプレス(0)になった自動分岐は、
                                // 再出現時にまず .s として復活させる。
                                new_cur_size = Some(crate::symbol::types::SizeCode::Short);
                            } else if can_shrink_to_short(*cur_size, loc, ev.value as u32, offset) {
                                // HAS互換: 前方分岐では短縮に伴ってターゲットも前詰めされるため、
                                // w→s で +2, l→s で +4 まで許容される。
                                new_cur_size = Some(crate::symbol::types::SizeCode::Short);
                            } else {
                                new_cur_size = None; // word
                            }
                        } else {
                            new_cur_size = None; // セクション違いは最適化しない
                        }
                    } else {
                        new_cur_size = None; // 未確定は最適化しない
                    }
                } else {
                    // 明示サイズはサプレスしない
                    new_cur_size = *req_size;
                }

                if *cur_size != new_cur_size || *suppressed != new_suppressed {
                    *cur_size = new_cur_size;
                    *suppressed = new_suppressed;
                    changed = true;
                }
                if !new_suppressed {
                    loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(branch_word_size(new_cur_size));
                }
            }
            rec => {
                let sz = rec.byte_size();
                if sz > 0 {
                    loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(sz);
                }
            }
        }
    }
    changed
}

/// 分岐命令がショート形式に縮小できるか判定する
/// target を評価し、オフセットが [-128, 127] に収まれば true
fn eval_target(sym: &SymbolTable, target: &Rpn, loc: u32, sect_id: u8) -> Option<EvalValue> {
    let result = eval_rpn(target, loc, loc, sect_id, &|name| {
        sym.lookup_sym(name).and_then(|s| {
            if let Symbol::Value { value, section, attrib, .. } = s {
                if *attrib >= DefAttrib::NoDet {
                    return Some(EvalValue { value: *value, section: *section });
                }
            }
            None
        })
    });
    result.ok()
}

fn is_bsr(opcode: u16) -> bool {
    // 条件コード部が 0001 なら BSR
    ((opcode >> 8) & 0x0f) == 0x01
}

fn can_shrink_to_short(
    cur_size: Option<crate::symbol::types::SizeCode>,
    branch_loc: u32,
    target_addr: u32,
    raw_offset: i64,
) -> bool {
    let old_size = branch_word_size(cur_size) as i64;
    let shrink = old_size - 2;
    let forward = target_addr > branch_loc;
    let adjusted = if forward && shrink > 0 { raw_offset - shrink } else { raw_offset };
    adjusted >= -128 && adjusted <= 127
}

fn estimate_deferred_size(
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

    let resolved_ops: Vec<EffectiveAddress> = ops.iter()
        .map(|ea| resolve_ea_for_pass2(sym, ea, loc, sect))
        .collect();

    match encode_insn(base, handler, size, &resolved_ops) {
        Ok(bytes) => bytes.len() as u32,
        Err(InsnError::DeferToLinker) => 2 + resolved_ops.iter().map(|ea| ea_ext_size_for_insn(ea, size)).sum::<u32>(),
        Err(_) => 2 + resolved_ops.iter().map(|ea| ea_ext_size_for_insn(ea, size)).sum::<u32>(),
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
        _ => ea.clone(),
    }
}

fn eval_rpn_with_sym(sym: &SymbolTable, rpn: &Rpn, loc: u32, sect: u8) -> Option<EvalValue> {
    eval_rpn(rpn, loc, loc, sect, &|name| {
        sym.lookup_sym(name).and_then(|s| {
            if let Symbol::Value { value, section, attrib, .. } = s {
                if *attrib >= DefAttrib::NoDet {
                    return Some(EvalValue { value: *value, section: *section });
                }
            }
            None
        })
    }).ok()
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
        EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_)
        | EffectiveAddress::AddrRegInd(_) | EffectiveAddress::AddrRegPostInc(_)
        | EffectiveAddress::AddrRegPreDec(_) => 0,
        EffectiveAddress::AbsShort(_) | EffectiveAddress::AddrRegDisp { .. }
        | EffectiveAddress::PcDisp(_) => 2,
        EffectiveAddress::AbsLong(_) => 4,
        EffectiveAddress::Immediate(_) => 2,
        EffectiveAddress::AddrRegIdx { .. } | EffectiveAddress::PcIdx { .. } => 2,
        EffectiveAddress::CcrReg | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_) | EffectiveAddress::FpCtrlReg(_) => 0,
    }
}

fn update_symbol(sym: &mut SymbolTable, name: &[u8], section: u8, value: i32) {
    use crate::symbol::types::{ExtAttrib, FirstDef};
    // 既存シンボルの ext_attrib を保持する（:: ラベルの XDEF 属性が消えないように）
    let ext_attrib = if let Some(Symbol::Value { ext_attrib, .. }) = sym.lookup_sym(name) {
        *ext_attrib
    } else {
        ExtAttrib::None
    };
    let new_sym = Symbol::Value {
        attrib:     DefAttrib::Define,
        ext_attrib,
        section,
        org_num:    0,
        first:      FirstDef::Other,
        opt_count:  0,
        value,
    };
    sym.define(name.to_vec(), new_sym);
}
