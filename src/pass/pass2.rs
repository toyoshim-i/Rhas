/// Pass 2: 最適化（分岐サイズ縮小）
///
/// オリジナルの pass2 に対応。分岐命令のサイズを最適化する。
/// 収束するまで繰り返し分岐サイズを縮小する。

use crate::expr::{eval_rpn, Rpn};
use crate::expr::eval::EvalValue;
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::DefAttrib;
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
            TempRecord::Org { value } => {
                loc_ctr[cur_sect] = *value;
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
                            } else if offset >= -128 && offset <= 127 {
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
