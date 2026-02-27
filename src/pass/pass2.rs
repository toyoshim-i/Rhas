/// Pass 2: 最適化（分岐サイズ縮小）
///
/// オリジナルの pass2 に対応。分岐命令のサイズを最適化する。
/// 収束するまで繰り返し分岐サイズを縮小する。

use crate::expr::{eval_rpn, Rpn};
use crate::expr::eval::EvalValue;
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, SizeCode};
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
            TempRecord::Branch { req_size, target, .. } => {
                let loc = loc_ctr[cur_sect];
                // .w（デフォルト）形式のみ縮小候補
                if req_size.is_none() || *req_size == Some(SizeCode::Word) {
                    if try_shrink_branch(sym, target, loc, cur_sect as u8 + 1) {
                        *req_size = Some(SizeCode::Short);
                        changed = true;
                    }
                }
                loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(branch_word_size(*req_size));
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
fn try_shrink_branch(sym: &SymbolTable, target: &Rpn, loc: u32, sect_id: u8) -> bool {
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
    match result {
        Ok(v) if v.section as u8 == sect_id => {
            // 同一セクション内の参照: オフセットを計算
            // 68000: offset = target - (branch_pc + 2)
            let branch_end = loc.wrapping_add(2);
            let offset = (v.value as i64) - (branch_end as i64);
            offset >= -128 && offset <= 127
        }
        _ => false,
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
