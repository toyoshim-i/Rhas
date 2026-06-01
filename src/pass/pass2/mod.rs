//! Pass 2: 最適化（分岐サイズ縮小 + DeferredInsn サイズ再評価）
//!
//! オリジナルの pass2 に対応。分岐命令のサイズ最適化に加えて、
//! Pass1 で未解決だった EA を再評価して命令長を更新する。
//! 収束するまで繰り返す。

use super::temp::{branch_word_size, TempRecord};
use crate::symbol::types::{DefAttrib, SizeCode};
use crate::symbol::{Symbol, SymbolTable};

mod branch;
mod eval;

use branch::{can_shrink_to_short, eval_target, is_bsr};
use eval::{estimate_deferred_size, eval_rpn_with_sym};

/// Pass2: TempRecord 列のロケーションカウンタを再計算し、分岐サイズを最適化する
///
/// 1. ロケーションカウンタを再計算してシンボルを更新する
/// 2. 最適化できる分岐（.w → .s）を縮小する
/// 3. 収束するまで繰り返す
pub fn pass2(records: &mut [TempRecord], sym: &mut SymbolTable) {
    // 最大反復回数（通常は少ない繰り返しで収束する）
    for _ in 0..32 {
        let changed = pass2_one(records, sym);
        if !changed {
            break;
        }
    }
}

/// 1回のパス: ロケーションカウンタ再計算 + 分岐縮小
/// 変化があれば true を返す
fn pass2_one(records: &mut [TempRecord], sym: &mut SymbolTable) -> bool {
    let mut loc_ctr = [0u32; 10];
    let mut cur_sect = 0usize;
    let mut changed = false;

    for rec in records.iter_mut() {
        match rec {
            TempRecord::SectChange { id } => {
                let idx = (*id as usize).saturating_sub(1);
                if idx < 10 {
                    cur_sect = idx;
                }
            }
            TempRecord::Align { n, section, .. } => {
                let idx = (*section as usize).saturating_sub(1).min(9);
                let align = 1u32 << *n;
                let loc = loc_ctr[idx];
                let new_loc = (loc + align - 1) & !(align - 1);
                loc_ctr[idx] = new_loc;
            }
            TempRecord::LabelDef {
                name,
                section,
                offset,
            } => {
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
            TempRecord::DeferredInsn {
                base,
                handler,
                size,
                ops,
                byte_size,
            } => {
                let loc = loc_ctr[cur_sect];
                let sect = cur_sect as u8 + 1;
                let new_size = estimate_deferred_size(sym, *base, *handler, *size, ops, loc, sect);
                if *byte_size != new_size {
                    *byte_size = new_size;
                    changed = true;
                }
                loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(*byte_size);
            }
            TempRecord::Branch {
                opcode,
                req_size,
                cur_size,
                suppressed,
                target,
            } => {
                let loc = loc_ctr[cur_sect];
                let (new_cur_size, new_suppressed) = if req_size.is_none() {
                    if let Some(ev) = eval_target(sym, target, loc, cur_sect as u8 + 1) {
                        // 同一セクション参照のみ最適化対象
                        if ev.section == cur_sect as u8 + 1 {
                            let offset = (ev.value as i64) - (loc as i64 + 2);
                            let next_offset = if *suppressed {
                                -2
                            } else {
                                match cur_size {
                                    Some(SizeCode::Short) => 0,
                                    Some(SizeCode::Long) => 4,
                                    _ => 2, // None=.w
                                }
                            };
                            // 直後への bra/bcc は命令自体を削除（bsr は除外）
                            if offset == next_offset && !is_bsr(*opcode) {
                                (None, true)
                            } else if *suppressed {
                                // HAS互換: 一度サプレス(0)になった自動分岐は、
                                // 再出現時にまず .s として復活させる。
                                (Some(SizeCode::Short), false)
                            } else if can_shrink_to_short(*cur_size, loc, ev.value as u32, offset) {
                                // HAS互換: 前方分岐では短縮に伴ってターゲットも前詰めされるため、
                                // w→s で +2, l→s で +4 まで許容される。
                                (Some(SizeCode::Short), false)
                            } else {
                                (None, false) // word
                            }
                        } else {
                            (None, false) // セクション違いは最適化しない
                        }
                    } else {
                        (None, false) // 未確定は最適化しない
                    }
                } else {
                    // 明示サイズはサプレスしない
                    (*req_size, false)
                };

                if *cur_size != new_cur_size || *suppressed != new_suppressed {
                    *cur_size = new_cur_size;
                    *suppressed = new_suppressed;
                    changed = true;
                }
                if !new_suppressed {
                    loc_ctr[cur_sect] =
                        loc_ctr[cur_sect].wrapping_add(branch_word_size(new_cur_size));
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

fn update_symbol(sym: &mut SymbolTable, name: &[u8], section: u8, value: i32) {
    use crate::symbol::types::{ExtAttrib, FirstDef};
    // 既存シンボルの ext_attrib を保持する（:: ラベルの XDEF 属性が消えないように）
    let ext_attrib = if let Some(Symbol::Value { ext_attrib, .. }) = sym.lookup_sym(name) {
        *ext_attrib
    } else {
        ExtAttrib::None
    };
    let new_sym = Symbol::Value {
        attrib: DefAttrib::Define,
        ext_attrib,
        section,
        org_num: 0,
        first: FirstDef::Other,
        opt_count: 0,
        value,
    };
    sym.define(name.to_vec(), new_sym);
}
