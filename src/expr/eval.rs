/// RPN 式の評価器
///
/// オリジナルの `calcrpn`（expr.s）に対応する。
/// シンボルテーブル・ロケーションカウンタはクロージャで渡す設計とし、
/// モジュール間の循環依存を避ける。

use super::rpn::{Operator, RPNToken, Rpn};

// ----------------------------------------------------------------
// 評価結果型
// ----------------------------------------------------------------

/// RPN 評価の結果値
///
/// オリジナルでは (d1.l=値, d0.w=属性) という形で返す。
/// 属性（section）が 0 なら定数、1〜10 はセクション番号、
/// $01FF はオフセット付き外部参照、$02FF は外部参照値。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalValue {
    /// 値（32bit）
    pub value: i32,
    /// セクション番号（0=定数、1〜=アドレス値）
    pub section: u8,
}

impl EvalValue {
    pub const fn constant(value: i32) -> Self {
        EvalValue { value, section: 0 }
    }

    pub fn is_constant(self) -> bool {
        self.section == 0
    }
}

/// 評価エラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    /// 0 除算
    DivisionByZero,
    /// 未定義シンボル
    UndefinedSymbol(Vec<u8>),
    /// 数値オーバーフロー
    Overflow,
    /// 定数でない演算をリンカに持ち越す（エラーではなく保留）
    DeferToLinker,
}

// ----------------------------------------------------------------
// 評価器
// ----------------------------------------------------------------

/// RPN 式を評価する
///
/// * `rpn` - 評価対象の RPN トークン列
/// * `loc` - 行頭ロケーションカウンタ（'*' の値）
/// * `cur_loc` - 現在のロケーションカウンタ（'$' の値）
/// * `section` - 現在のセクション番号（ロケーション属性として使用）
/// * `lookup` - シンボル名→EvalValue のルックアップ関数
///              未定義なら None を返す
pub fn eval_rpn(
    rpn: &Rpn,
    loc: u32,
    cur_loc: u32,
    section: u8,
    lookup: &dyn Fn(&[u8]) -> Option<EvalValue>,
) -> Result<EvalValue, EvalError> {
    // スタック（value, section の組）
    let mut stack: Vec<EvalValue> = Vec::with_capacity(16);

    for token in rpn {
        match token {
            RPNToken::End => break,

            // ---- 即値 ----
            RPNToken::ValueByte(v) => stack.push(EvalValue::constant(*v as i32)),
            RPNToken::ValueWord(v) => stack.push(EvalValue::constant(*v as i16 as i32)),
            RPNToken::Value(v)     => stack.push(EvalValue::constant(*v as i32)),

            // ---- シンボル参照 ----
            RPNToken::SymbolRef(name) => {
                match lookup(name) {
                    Some(val) => stack.push(val),
                    None      => return Err(EvalError::UndefinedSymbol(name.clone())),
                }
            }

            // ---- ロケーションカウンタ ----
            RPNToken::Location    => stack.push(EvalValue { value: loc as i32, section }),
            RPNToken::CurrentLoc  => stack.push(EvalValue { value: cur_loc as i32, section }),

            // ---- 演算子 ----
            RPNToken::Op(op) => {
                eval_op(*op, &mut stack)?;
            }
        }
    }

    stack.pop().ok_or(EvalError::Overflow)
}

// ----------------------------------------------------------------
// 演算子処理
// ----------------------------------------------------------------

fn eval_op(op: Operator, stack: &mut Vec<EvalValue>) -> Result<(), EvalError> {
    if op.is_unary() {
        let a = stack.pop().ok_or(EvalError::Overflow)?;
        let result = apply_unary(op, a)?;
        stack.push(result);
    } else {
        // 二項: スタックから A（右辺）、B（左辺）の順で取り出す
        let a = stack.pop().ok_or(EvalError::Overflow)?;
        let b = stack.pop().ok_or(EvalError::Overflow)?;
        let result = apply_binary(op, b, a)?;
        stack.push(result);
    }
    Ok(())
}

/// 単項演算子の適用（オリジナルの _neg〜_nul に対応）
fn apply_unary(op: Operator, a: EvalValue) -> Result<EvalValue, EvalError> {
    // 定数以外への単項演算はリンカに持ち越す（オリジナルの calcrpnref）
    if !a.is_constant() {
        return Err(EvalError::DeferToLinker);
    }
    let v = a.value;
    let result = match op {
        Operator::Neg  => v.wrapping_neg(),
        Operator::Pos  => v,
        Operator::Not  => !v,
        Operator::High => ((v as u32 >> 8) & 0xFF) as i32,
        Operator::Low  => (v as u32 & 0xFF) as i32,
        Operator::HighW => ((v as u32 >> 16) & 0xFFFF) as i32,
        Operator::LowW  => (v as u32 & 0xFFFF) as i32,
        Operator::Nul  => 0,
        _ => unreachable!(),
    };
    Ok(EvalValue::constant(result))
}

/// 二項演算子の適用（オリジナルの _mul〜_or に対応）
///
/// `b` は左辺（先にスタックに積まれた値）、`a` は右辺（後）。
fn apply_binary(op: Operator, b: EvalValue, a: EvalValue) -> Result<EvalValue, EvalError> {
    match op {
        Operator::Add => apply_add(b, a),
        Operator::Sub => apply_sub(b, a),
        _ => {
            // 加算・減算以外は定数同士でなければリンカに持ち越す
            if b.section != 0 || a.section != 0 {
                return Err(EvalError::DeferToLinker);
            }
            let bv = b.value;
            let av = a.value;
            let result = match op {
                Operator::Mul => mul32(bv, av),
                Operator::Div => {
                    if av == 0 { return Err(EvalError::DivisionByZero); }
                    div32(bv, av)
                }
                Operator::Mod => {
                    if av == 0 { return Err(EvalError::DivisionByZero); }
                    mod32(bv, av)
                }
                Operator::Shr => ((bv as u32).wrapping_shr(av as u32)) as i32,
                Operator::Shl => ((bv as u32).wrapping_shl(av as u32)) as i32,
                Operator::Asr => bv.wrapping_shr(av as u32),
                Operator::Eq  => bool_to_i32(bv == av),
                Operator::Ne  => bool_to_i32(bv != av),
                Operator::Lt  => bool_to_i32((bv as u32) < (av as u32)),
                Operator::Le  => bool_to_i32((bv as u32) <= (av as u32)),
                Operator::Gt  => bool_to_i32((bv as u32) > (av as u32)),
                Operator::Ge  => bool_to_i32((bv as u32) >= (av as u32)),
                Operator::Slt => bool_to_i32(bv < av),
                Operator::Sle => bool_to_i32(bv <= av),
                Operator::Sgt => bool_to_i32(bv > av),
                Operator::Sge => bool_to_i32(bv >= av),
                Operator::And => bv & av,
                Operator::Xor => bv ^ av,
                Operator::Or  => bv | av,
                Operator::Add | Operator::Sub => unreachable!(),
                _ => unreachable!(),
            };
            Ok(EvalValue::constant(result))
        }
    }
}

/// 加算（オリジナルの _add に対応）
///
/// <定数>+<定数>、<アドレス>+<定数>、<定数>+<アドレス> が許容される。
/// <アドレス>+<アドレス>、<外部>+<??>はリンカに持ち越す。
fn apply_add(b: EvalValue, a: EvalValue) -> Result<EvalValue, EvalError> {
    if b.section == 0 && a.section == 0 {
        // <定数>+<定数>
        return Ok(EvalValue::constant(b.value.wrapping_add(a.value)));
    }
    if b.section != 0 && a.section != 0 {
        // <アドレス>+<アドレス> → リンカに持ち越す
        return Err(EvalError::DeferToLinker);
    }
    // <アドレス>+<定数> または <定数>+<アドレス>
    let addr = if b.section != 0 { b } else { a };
    let cst  = if b.section == 0 { b } else { a };
    Ok(EvalValue {
        value:   addr.value.wrapping_add(cst.value),
        section: addr.section,
    })
}

/// 減算（オリジナルの _sub に対応）
///
/// <定数>-<定数>、<アドレス>-<定数>、同一セクション <アドレス>-<アドレス> が許容される。
fn apply_sub(b: EvalValue, a: EvalValue) -> Result<EvalValue, EvalError> {
    if b.section == 0 && a.section == 0 {
        // <定数>-<定数>
        return Ok(EvalValue::constant(b.value.wrapping_sub(a.value)));
    }
    if b.section != 0 && a.section != 0 {
        if b.section == a.section {
            // 同一セクション <アドレス>-<アドレス> → 定数
            return Ok(EvalValue::constant(b.value.wrapping_sub(a.value)));
        }
        // 異なるセクション → リンカに持ち越す
        return Err(EvalError::DeferToLinker);
    }
    if b.section != 0 && a.section == 0 {
        // <アドレス>-<定数>
        return Ok(EvalValue {
            value:   b.value.wrapping_sub(a.value),
            section: b.section,
        });
    }
    // <定数>-<アドレス> → リンカに持ち越す
    Err(EvalError::DeferToLinker)
}

// ----------------------------------------------------------------
// 算術ヘルパー
// ----------------------------------------------------------------

/// 32bit 乗算（符号付き、オーバーフローは捨てる）
fn mul32(a: i32, b: i32) -> i32 {
    // オリジナルは 32bit × 32bit → 32bit（下位 32bit のみ使用）
    a.wrapping_mul(b)
}

/// 32bit 除算（符号付き切り捨て、オリジナルの _div に対応）
fn div32(a: i32, b: i32) -> i32 {
    // オリジナルは符号を手動処理してから符号なし除算
    // Rust の `wrapping_div` は同等の結果
    if b == 0 { return 0; }
    a.wrapping_div(b)
}

/// 32bit 剰余（符号付き、オリジナルの _mod に対応）
fn mod32(a: i32, b: i32) -> i32 {
    if b == 0 { return 0; }
    a.wrapping_rem(b)
}

/// bool → i32（true=-1, false=0、オリジナルの seq/sne 等に合わせる）
#[inline]
fn bool_to_i32(v: bool) -> i32 {
    if v { -1 } else { 0 }
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::rpn::RPNToken as T;

    fn no_lookup(_: &[u8]) -> Option<EvalValue> { None }

    fn sym_lookup(name: &[u8]) -> Option<EvalValue> {
        if name == b"X" { Some(EvalValue { value: 0x1000, section: 1 }) }
        else if name == b"Y" { Some(EvalValue { value: 0x2000, section: 1 }) }
        else if name == b"C" { Some(EvalValue::constant(42)) }
        else { None }
    }

    fn eval(rpn: Rpn) -> Result<EvalValue, EvalError> {
        eval_rpn(&rpn, 0x100, 0x200, 1, &no_lookup)
    }

    #[test]
    fn test_constant() {
        let rpn = vec![T::Value(42), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(42)));
    }

    #[test]
    fn test_add_constants() {
        // 1 + 2 → 3
        let rpn = vec![T::ValueByte(1), T::ValueByte(2), T::Op(Operator::Add), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(3)));
    }

    #[test]
    fn test_mul_constants() {
        // 3 * 4 = 12
        let rpn = vec![T::ValueByte(3), T::ValueByte(4), T::Op(Operator::Mul), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(12)));
    }

    #[test]
    fn test_unary_neg() {
        // -(5) = -5
        let rpn = vec![T::ValueByte(5), T::Op(Operator::Neg), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(-5)));
    }

    #[test]
    fn test_unary_not() {
        // ~1 = -2 (0xFFFFFFFE として)
        let rpn = vec![T::ValueByte(1), T::Op(Operator::Not), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(-2)));
    }

    #[test]
    fn test_high_low() {
        // .high. $1234 = $12
        let rpn = vec![T::ValueWord(0x1234), T::Op(Operator::High), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(0x12)));
        // .low. $1234 = $34
        let rpn = vec![T::ValueWord(0x1234), T::Op(Operator::Low), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(0x34)));
    }

    #[test]
    fn test_highw_loww() {
        // .highw. $12345678 = $1234
        let rpn = vec![T::Value(0x12345678), T::Op(Operator::HighW), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(0x1234)));
        // .loww. $12345678 = $5678
        let rpn = vec![T::Value(0x12345678), T::Op(Operator::LowW), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(0x5678)));
    }

    #[test]
    fn test_div_zero() {
        let rpn = vec![T::ValueByte(1), T::ValueByte(0), T::Op(Operator::Div), T::End];
        assert_eq!(eval(rpn), Err(EvalError::DivisionByZero));
    }

    #[test]
    fn test_comparison() {
        // 3 < 5 = -1 (true)
        let rpn = vec![T::ValueByte(3), T::ValueByte(5), T::Op(Operator::Slt), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(-1)));
        // 5 < 3 = 0 (false)
        let rpn = vec![T::ValueByte(5), T::ValueByte(3), T::Op(Operator::Slt), T::End];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(0)));
    }

    #[test]
    fn test_location() {
        // '*' = 0x100（loc）
        let rpn = vec![T::Location, T::End];
        let res = eval_rpn(&rpn, 0x100, 0x200, 1, &no_lookup);
        assert_eq!(res, Ok(EvalValue { value: 0x100, section: 1 }));
        // '$' = 0x200（cur_loc）
        let rpn = vec![T::CurrentLoc, T::End];
        let res = eval_rpn(&rpn, 0x100, 0x200, 1, &no_lookup);
        assert_eq!(res, Ok(EvalValue { value: 0x200, section: 1 }));
    }

    #[test]
    fn test_symbol_lookup() {
        let rpn = vec![T::SymbolRef(b"C".to_vec()), T::ValueByte(10), T::Op(Operator::Add), T::End];
        let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
        assert_eq!(res, Ok(EvalValue::constant(52)));
    }

    #[test]
    fn test_addr_plus_const() {
        // X + 4 → section=1, value=0x1004
        let rpn = vec![T::SymbolRef(b"X".to_vec()), T::ValueByte(4), T::Op(Operator::Add), T::End];
        let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
        assert_eq!(res, Ok(EvalValue { value: 0x1004, section: 1 }));
    }

    #[test]
    fn test_addr_minus_addr_same_section() {
        // X - Y → 0x1000 - 0x2000 = -0x1000 (constant, section=0)
        let rpn = vec![T::SymbolRef(b"X".to_vec()), T::SymbolRef(b"Y".to_vec()), T::Op(Operator::Sub), T::End];
        let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
        assert_eq!(res, Ok(EvalValue { value: -0x1000, section: 0 }));
    }

    #[test]
    fn test_undefined_symbol() {
        let rpn = vec![T::SymbolRef(b"UNDEF".to_vec()), T::End];
        let res = eval_rpn(&rpn, 0, 0, 0, &no_lookup);
        assert_eq!(res, Err(EvalError::UndefinedSymbol(b"UNDEF".to_vec())));
    }

    #[test]
    fn test_complex_expr() {
        // (3 + 4) * 2 = 14
        // RPN: 3 4 + 2 *
        let rpn = vec![
            T::ValueByte(3), T::ValueByte(4), T::Op(Operator::Add),
            T::ValueByte(2), T::Op(Operator::Mul),
            T::End,
        ];
        assert_eq!(eval(rpn), Ok(EvalValue::constant(14)));
    }
}
