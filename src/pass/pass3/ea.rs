use super::P3Ctx;
use crate::addressing::{Displacement, EffectiveAddress};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::{Operator, RPNToken};
use crate::expr::Rpn;
use crate::symbol::types::{DefAttrib, SizeCode};
use crate::symbol::{Symbol, SymbolTable};

/// EA に含まれる外部参照の種別
#[derive(Debug, Clone)]
pub(super) enum EaExtKind {
    /// シンプルな絶対外部参照: $41/$42 FF xref_num
    SimpleAbs(Vec<u8>),
    /// 外部参照 + 定数オフセット: $50/$51/$52 FF xref_num offset4 (ROFST形式)
    ExtWithOffset(Vec<u8>, i32),
    /// PC相対外部参照: $65 sect loc4 xref_num
    PcRel(Vec<u8>),
    /// 複合外部式: RPN フォーマット
    Complex(Rpn),
    /// セクション内絶対参照: $41/$42 sect value (同一セクション内ラベル参照)
    SectionAbs(u8),
}

#[derive(Clone)]
struct FoldExpr {
    name: Option<Vec<u8>>,
    offset: i32,
}

pub(super) fn sym_to_eval(sym: &Symbol) -> Option<EvalValue> {
    if let Symbol::Value {
        value,
        section,
        attrib,
        ..
    } = sym
    {
        if *attrib >= DefAttrib::NoDet {
            return Some(EvalValue {
                value: *value,
                section: *section,
            });
        }
    }
    None
}

/// RegSym エイリアスチェーンをたどって最終的なシンボル名を返す（所有値）
///
/// `abswarn reg abswarn2` の場合: "abswarn" → "abswarn2"
/// チェーンが RegSym でなくなったとき、またはループ上限に達したとき終了。
pub(super) fn resolve_regsym_chain(sym: &SymbolTable, name: &[u8]) -> Vec<u8> {
    let mut current_name: &[u8] = name;
    let mut depth = 0u8;
    loop {
        if depth >= 16 {
            break;
        }
        if let Some(Symbol::RegSym { define }) = sym.lookup_sym(current_name) {
            if let Some(rpn) = define.first() {
                if let [RPNToken::SymbolRef(target), RPNToken::End] = rpn.as_slice() {
                    current_name = target;
                    depth += 1;
                    continue;
                }
            }
        }
        break;
    }
    current_name.to_vec()
}

/// RPN がシンプルな外部参照 [SymbolRef(name), End] かチェック
pub(super) fn is_simple_external(rpn: &Rpn) -> Option<&Vec<u8>> {
    if rpn.len() == 2 {
        if let (RPNToken::SymbolRef(name), RPNToken::End) = (&rpn[0], &rpn[1]) {
            return Some(name);
        }
    }
    None
}

/// RPN が「単一 XREF + 定数オフセット」に簡約できるかチェック。
///
/// 例:
/// - `sym + 4`
/// - `4 + sym`
/// - `sym + (16*4)`  // 定数部分は先に畳み込む
pub(super) fn is_external_with_offset(rpn: &Rpn, sym: &SymbolTable) -> Option<(Vec<u8>, i32)> {
    let mut stack: Vec<FoldExpr> = Vec::new();

    for tok in rpn {
        match tok {
            RPNToken::End => break,
            RPNToken::Value(v) => stack.push(FoldExpr {
                name: None,
                offset: *v as i32,
            }),
            RPNToken::ValueWord(v) => stack.push(FoldExpr {
                name: None,
                offset: *v as i32,
            }),
            RPNToken::ValueByte(v) => stack.push(FoldExpr {
                name: None,
                offset: *v as i32,
            }),
            RPNToken::SymbolRef(name) => {
                if let Some(v) = sym
                    .lookup_sym(name)
                    .and_then(sym_to_eval)
                    .filter(|v| v.is_constant())
                {
                    stack.push(FoldExpr {
                        name: None,
                        offset: v.value,
                    });
                } else {
                    stack.push(FoldExpr {
                        name: Some(name.clone()),
                        offset: 0,
                    });
                }
            }
            RPNToken::Op(op) => {
                let rhs = stack.pop()?;
                let lhs = stack.pop()?;
                let merged = match op {
                    Operator::Add => fold_add(lhs, rhs)?,
                    Operator::Sub => fold_sub(lhs, rhs)?,
                    Operator::Mul => fold_mul(lhs, rhs)?,
                    _ => return None,
                };
                stack.push(merged);
            }
            _ => return None,
        }
    }
    let out = stack.pop()?;
    if !stack.is_empty() {
        return None;
    }
    out.name.map(|n| (n, out.offset))
}

fn fold_add(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr {
            name: None,
            offset: lhs.offset + rhs.offset,
        }),
        (Some(n), None) => Some(FoldExpr {
            name: Some(n),
            offset: lhs.offset + rhs.offset,
        }),
        (None, Some(n)) => Some(FoldExpr {
            name: Some(n),
            offset: lhs.offset + rhs.offset,
        }),
        (Some(_), Some(_)) => None,
    }
}

fn fold_sub(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr {
            name: None,
            offset: lhs.offset - rhs.offset,
        }),
        (Some(n), None) => Some(FoldExpr {
            name: Some(n),
            offset: lhs.offset - rhs.offset,
        }),
        _ => None,
    }
}

fn fold_mul(lhs: FoldExpr, rhs: FoldExpr) -> Option<FoldExpr> {
    match (lhs.name, rhs.name) {
        (None, None) => Some(FoldExpr {
            name: None,
            offset: lhs.offset * rhs.offset,
        }),
        // (sym + k) * 1 は恒等
        (Some(n), None) if rhs.offset == 1 => Some(FoldExpr {
            name: Some(n),
            offset: lhs.offset,
        }),
        (None, Some(n)) if lhs.offset == 1 => Some(FoldExpr {
            name: Some(n),
            offset: rhs.offset,
        }),
        _ => None,
    }
}

/// RPN 内の全 SymbolRef に対して try_register_xdef を呼ぶ（B2xx 順序の先行登録）
pub(super) fn register_xdefs_in_rpn(ctx: &mut P3Ctx<'_>, rpn: &Rpn) {
    for tok in rpn {
        if let RPNToken::SymbolRef(name) = tok {
            ctx.try_register_xdef(name);
        }
    }
}

/// EA 内の全 RPN に対して register_xdefs_in_rpn を呼ぶ
pub(super) fn register_xdefs_in_ea(ctx: &mut P3Ctx<'_>, ea: &EffectiveAddress) {
    match ea {
        EffectiveAddress::Immediate(rpn)
        | EffectiveAddress::AbsShort(rpn)
        | EffectiveAddress::AbsLong(rpn) => register_xdefs_in_rpn(ctx, rpn),
        EffectiveAddress::AddrRegDisp { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::AddrRegIdx { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::PcDisp(disp) => register_xdefs_in_rpn(ctx, &disp.rpn),
        EffectiveAddress::PcIdx { disp, .. } => register_xdefs_in_rpn(ctx, &disp.rpn),
        _ => {}
    }
}

/// EA 内の RPN 式を評価して定数 EA を返す。外部参照の場合は (EA_with_zero, Some(EaExtKind)) を返す。
pub(super) fn resolve_ea_with_ext(
    ctx: &mut P3Ctx<'_>,
    ea: &EffectiveAddress,
) -> (EffectiveAddress, Option<EaExtKind>) {
    let zero_rpn = || vec![RPNToken::Value(0u32), RPNToken::End];

    // RPN 評価を試み、外部参照の場合に EaExtKind を決定する
    let classify_ext = |rpn: &Rpn, sym: &SymbolTable| -> EaExtKind {
        if let Some(name) = is_simple_external(rpn) {
            EaExtKind::SimpleAbs(name.clone())
        } else if let Some((name, offset)) = is_external_with_offset(rpn, sym) {
            EaExtKind::ExtWithOffset(name.clone(), offset)
        } else {
            EaExtKind::Complex(rpn.clone())
        }
    };

    match ea {
        EffectiveAddress::Immediate(rpn) => match ctx.eval(rpn) {
            Ok(v) => (
                EffectiveAddress::Immediate(vec![RPNToken::Value(v.value as u32), RPNToken::End]),
                None,
            ),
            Err(_) => {
                let kind = classify_ext(rpn, ctx.sym);
                (
                    EffectiveAddress::Immediate(zero_rpn()),
                    Some(kind),
                )
            }
        },
        EffectiveAddress::AbsShort(rpn) => match ctx.eval(rpn) {
            Ok(v) if v.section != 0 => (
                EffectiveAddress::AbsShort(vec![RPNToken::Value(v.value as u32), RPNToken::End]),
                Some(EaExtKind::SectionAbs(v.section)),
            ),
            Ok(v) => (
                EffectiveAddress::AbsShort(vec![RPNToken::Value(v.value as u32), RPNToken::End]),
                None,
            ),
            Err(name) => (
                EffectiveAddress::AbsShort(zero_rpn()),
                Some(EaExtKind::SimpleAbs(name)),
            ),
        },
        EffectiveAddress::AbsLong(rpn) => match ctx.eval(rpn) {
            Ok(v) if v.section != 0 => (
                EffectiveAddress::AbsLong(vec![RPNToken::Value(v.value as u32), RPNToken::End]),
                Some(EaExtKind::SectionAbs(v.section)),
            ),
            Ok(v) => (
                EffectiveAddress::AbsLong(vec![RPNToken::Value(v.value as u32), RPNToken::End]),
                None,
            ),
            Err(name) => (
                EffectiveAddress::AbsLong(zero_rpn()),
                Some(EaExtKind::SimpleAbs(name)),
            ),
        },
        EffectiveAddress::AddrRegDisp { an, disp } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(v.value as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(v.value),
                        };
                        (
                            EffectiveAddress::AddrRegDisp {
                                an: *an,
                                disp: new_disp,
                            },
                            None,
                        )
                    }
                    Err(_) => {
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            // Note: Use non-zero placeholder to prevent (0,An)→(An) optimization.
                            // The actual value doesn't matter since the relocation record overwrites it.
                            const_val: Some(1),
                        };
                        (
                            EffectiveAddress::AddrRegDisp {
                                an: *an,
                                disp: new_disp,
                            },
                            Some(classify_ext(&disp.rpn, ctx.sym)),
                        )
                    }
                }
            }
        }
        EffectiveAddress::PcDisp(disp) => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        // displacement = target_addr - displacement_word_addr
                        // displacement_word_addr = 命令先頭 + 2 (オペコードワード分)
                        let target_addr = v.value;
                        let disp_word_addr = ctx.location() as i32 + 2;
                        let displacement = target_addr - disp_word_addr;
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(displacement),
                        };
                        (EffectiveAddress::PcDisp(new_disp), None)
                    }
                    Err(name) => {
                        // PC相対外部参照: displacement=0 でエンコード、$65 リロケーションを生成
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            const_val: Some(0),
                        };
                        (
                            EffectiveAddress::PcDisp(new_disp),
                            Some(EaExtKind::PcRel(name)),
                        )
                    }
                }
            }
        }
        EffectiveAddress::PcIdx { disp, idx } => {
            if disp.const_val.is_some() || disp.rpn.is_empty() {
                (ea.clone(), None)
            } else {
                match ctx.eval(&disp.rpn) {
                    Ok(v) => {
                        // displacement = target_addr - displacement_word_addr (8bit)
                        let target_addr = v.value;
                        let disp_word_addr = ctx.location() as i32 + 2;
                        let displacement = target_addr - disp_word_addr;
                        let new_disp = Displacement {
                            rpn: vec![RPNToken::Value(displacement as u32), RPNToken::End],
                            size: disp.size,
                            const_val: Some(displacement),
                        };
                        (
                            EffectiveAddress::PcIdx {
                                disp: new_disp,
                                idx: *idx,
                            },
                            None,
                        )
                    }
                    Err(name) => {
                        let new_disp = Displacement {
                            rpn: zero_rpn(),
                            size: disp.size,
                            const_val: Some(0),
                        };
                        (
                            EffectiveAddress::PcIdx {
                                disp: new_disp,
                                idx: *idx,
                            },
                            Some(EaExtKind::PcRel(name)),
                        )
                    }
                }
            }
        }
        EffectiveAddress::PcMemIndPost { bd, idx, od } => {
            let resolve_disp = |ctx: &mut P3Ctx<'_>, d: &Displacement, is_pc_rel: bool| -> Displacement {
                if d.const_val.is_some() || d.rpn.is_empty() {
                    return d.clone();
                }
                match ctx.eval(&d.rpn) {
                    Ok(v) => {
                        let val = if is_pc_rel {
                            // BD = target_addr - ext_word_addr; ext_word is at PC+2
                            v.value - (ctx.location() as i32 + 2)
                        } else {
                            v.value
                        };
                        Displacement {
                            rpn: vec![RPNToken::Value(val as u32), RPNToken::End],
                            size: d.size,
                            const_val: Some(val),
                        }
                    }
                    Err(_) => d.clone(),
                }
            };
            let new_bd = resolve_disp(ctx, bd, true);
            let new_od = resolve_disp(ctx, od, false);
            (
                EffectiveAddress::PcMemIndPost {
                    bd: new_bd,
                    idx: *idx,
                    od: new_od,
                },
                None,
            )
        }
        EffectiveAddress::PcMemIndPre { bd, idx, od } => {
            let resolve_disp = |ctx: &mut P3Ctx<'_>, d: &Displacement, is_pc_rel: bool| -> Displacement {
                if d.const_val.is_some() || d.rpn.is_empty() {
                    return d.clone();
                }
                match ctx.eval(&d.rpn) {
                    Ok(v) => {
                        let val = if is_pc_rel {
                            v.value - (ctx.location() as i32 + 2)
                        } else {
                            v.value
                        };
                        Displacement {
                            rpn: vec![RPNToken::Value(val as u32), RPNToken::End],
                            size: d.size,
                            const_val: Some(val),
                        }
                    }
                    Err(_) => d.clone(),
                }
            };
            let new_bd = resolve_disp(ctx, bd, true);
            let new_od = resolve_disp(ctx, od, false);
            (
                EffectiveAddress::PcMemIndPre {
                    bd: new_bd,
                    idx: *idx,
                    od: new_od,
                },
                None,
            )
        }
        other => (other.clone(), None),
    }
}

/// 命令サイズを考慮した EA 拡張バイト数（Immediate は .l のとき 4 バイト）
pub(super) fn ea_ext_size_for_insn(ea: &EffectiveAddress, size: SizeCode) -> u32 {
    match ea {
        EffectiveAddress::Immediate(_) => match size {
            SizeCode::Long => 4,
            _ => 2,
        },
        other => ea_ext_size(other),
    }
}

pub(super) fn ea_ext_size(ea: &EffectiveAddress) -> u32 {
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
