/// Pass 2: 最適化（分岐サイズ縮小）
///
/// オリジナルの pass2 に対応。分岐命令のサイズを最適化する。
/// 現フェーズでは簡易版として、シンボル値の再計算のみ行う。
/// 分岐サイズ最適化は将来実装予定。

use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::DefAttrib;
use super::temp::TempRecord;

/// Pass2: TempRecord 列のロケーションカウンタを再計算してシンボルを更新する
///
/// Pass1 で記録した LabelDef の offset は Pass1 時点の推定値。
/// ここで再計算して更新する（分岐サイズ変化がある場合に必要）。
///
/// 現フェーズでは1回だけ通して確定（最適化なし）。
pub fn pass2(records: &mut Vec<TempRecord>, sym: &mut SymbolTable) {
    // 各セクションのロケーションカウンタ（最大 10 セクション）
    let mut loc_ctr = [0u32; 10];
    let mut cur_sect = 0usize; // text = section 1 → index 0

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
                let idx = (*section as usize).saturating_sub(1).min(9);
                let new_offset = loc_ctr[idx];
                *offset = new_offset;
                // シンボルテーブルの値を更新
                update_symbol(sym, name, *section, new_offset as i32);
            }
            TempRecord::Org { value } => {
                loc_ctr[cur_sect] = *value;
            }
            rec => {
                let sz = rec.byte_size();
                if sz > 0 {
                    loc_ctr[cur_sect] = loc_ctr[cur_sect].wrapping_add(sz);
                }
            }
        }
    }
}

fn update_symbol(sym: &mut SymbolTable, name: &[u8], section: u8, value: i32) {
    // SymbolTable::define() で上書き（既存シンボルを新しい値で再定義）
    use crate::symbol::types::{ExtAttrib, FirstDef};
    let new_sym = Symbol::Value {
        attrib:     DefAttrib::Define,
        ext_attrib: ExtAttrib::None,
        section,
        org_num:    0,
        first:      FirstDef::Other,
        opt_count:  0,
        value,
    };
    sym.define(name.to_vec(), new_sym);
}
