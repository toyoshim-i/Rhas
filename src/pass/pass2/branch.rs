use crate::expr::eval::EvalValue;
use crate::expr::{eval_rpn, Rpn};
use crate::pass::temp::branch_word_size;
use crate::symbol::types::DefAttrib;
use crate::symbol::{Symbol, SymbolTable};

/// 分岐命令がショート形式に縮小できるか判定する
/// target を評価し、オフセットが [-128, 127] に収まれば true
pub(super) fn eval_target(
    sym: &SymbolTable,
    target: &Rpn,
    loc: u32,
    sect_id: u8,
) -> Option<EvalValue> {
    let result = eval_rpn(target, loc, loc, sect_id, &|name| {
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
    });
    result.ok()
}

pub(super) fn is_bsr(opcode: u16) -> bool {
    // 条件コード部が 0001 なら BSR
    ((opcode >> 8) & 0x0f) == 0x01
}

pub(super) fn can_shrink_to_short(
    cur_size: Option<crate::symbol::types::SizeCode>,
    branch_loc: u32,
    target_addr: u32,
    raw_offset: i64,
) -> bool {
    let old_size = branch_word_size(cur_size) as i64;
    let shrink = old_size - 2;
    let forward = target_addr > branch_loc;
    let adjusted = if forward && shrink > 0 {
        raw_offset - shrink
    } else {
        raw_offset
    };
    (-128..=127).contains(&adjusted)
}
