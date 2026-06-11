use super::eval::{eval_rpn, EvalError, EvalValue};
use super::parse::parse_expr;
use super::rpn::{Operator, RPNToken, Rpn};
use super::ParseError;

use RPNToken as T;

// =================================================================
// mod.rs テスト群 (式パーサー・評価)
// =================================================================

fn parse(s: &str) -> Result<Rpn, ParseError> {
    let mut pos = 0;
    parse_expr(s.as_bytes(), &mut pos)
}

fn eval_const(rpn: Rpn) -> i32 {
    eval_rpn(&rpn, 0, 0, 0, &|_| None).unwrap().value
}

#[test]
fn test_decimal() {
    let rpn = parse("42").unwrap();
    assert_eq!(eval_const(rpn), 42);
}

#[test]
fn test_hex() {
    assert_eq!(eval_const(parse("$1A").unwrap()), 0x1A);
    assert_eq!(eval_const(parse("0xFF").unwrap()), 0xFF);
}

#[test]
fn test_octal() {
    assert_eq!(eval_const(parse("@17").unwrap()), 0o17);
}

#[test]
fn test_binary() {
    assert_eq!(eval_const(parse("%1010").unwrap()), 10);
}

#[test]
fn test_char_const() {
    assert_eq!(eval_const(parse("'A'").unwrap()), b'A' as i32);
    assert_eq!(
        eval_const(parse("'AB'").unwrap()),
        ((b'A' as i32) << 8) | b'B' as i32
    );
}

#[test]
fn test_add() {
    assert_eq!(eval_const(parse("1+2").unwrap()), 3);
    assert_eq!(eval_const(parse("10 - 3").unwrap()), 7);
}

#[test]
fn test_mul_prec() {
    // 2 + 3 * 4 = 14（乗算が先）
    assert_eq!(eval_const(parse("2+3*4").unwrap()), 14);
    // (2+3)*4 = 20
    assert_eq!(eval_const(parse("(2+3)*4").unwrap()), 20);
}

#[test]
fn test_unary_neg() {
    assert_eq!(eval_const(parse("-5").unwrap()), -5);
    assert_eq!(eval_const(parse("-(3+4)").unwrap()), -7);
}

#[test]
fn test_unary_not() {
    // HAS互換: '~' はシンボル先頭文字であり NOT 演算子ではない
    // ビット反転は .NOT. を使う
    assert_eq!(eval_const(parse(".not. 0").unwrap()), -1);
    assert_eq!(eval_const(parse(".not. $FF").unwrap()), -256); // NOT($FF) = $FFFFFF00
}

#[test]
fn test_keyword_ops() {
    assert_eq!(eval_const(parse("8 .mod. 3").unwrap()), 2);
    assert_eq!(eval_const(parse("1 .shl. 4").unwrap()), 16);
    assert_eq!(eval_const(parse("16 .shr. 2").unwrap()), 4);
    assert_eq!(eval_const(parse("5 .and. 3").unwrap()), 1);
    assert_eq!(eval_const(parse("5 .or. 3").unwrap()), 7);
    assert_eq!(eval_const(parse("5 .xor. 3").unwrap()), 6);
}

#[test]
fn test_comparisons() {
    assert_eq!(eval_const(parse("3 .slt. 5").unwrap()), -1);
    assert_eq!(eval_const(parse("5 .slt. 3").unwrap()), 0);
    assert_eq!(eval_const(parse("3 = 3").unwrap()), -1);
    assert_eq!(eval_const(parse("3 <> 4").unwrap()), -1);
    assert_eq!(eval_const(parse("3 != 4").unwrap()), -1);
}

#[test]
fn test_high_low() {
    assert_eq!(eval_const(parse(".high. $1234").unwrap()), 0x12);
    assert_eq!(eval_const(parse(".low. $1234").unwrap()), 0x34);
}

#[test]
fn test_symbol_ref() {
    let rpn = parse("LABEL+4").unwrap();
    let lookup = |name: &[u8]| {
        if name == b"LABEL" {
            Some(EvalValue {
                value: 0x1000,
                section: 1,
            })
        } else {
            None
        }
    };
    let res = eval_rpn(&rpn, 0, 0, 1, &lookup).unwrap();
    assert_eq!(
        res,
        EvalValue {
            value: 0x1004,
            section: 1
        }
    );
}

#[test]
fn test_left_assoc() {
    // 10 - 3 - 2 = (10-3)-2 = 5 （左結合）
    assert_eq!(eval_const(parse("10-3-2").unwrap()), 5);
}

#[test]
fn test_nested_parens() {
    // ((2+3)*(4-1)) = 15
    assert_eq!(eval_const(parse("((2+3)*(4-1))").unwrap()), 15);
}

#[test]
fn test_defined() {
    let rpn = parse(".defined. MYSYM").unwrap();
    // 未定義の場合は UndefinedSymbol エラーではなく、定義チェック
    // .defined. の特殊シンボル参照が生成されているか確認
    let has_defined = rpn.iter().any(|t| {
        if let RPNToken::SymbolRef(name) = t {
            name.starts_with(b"\x01defined\x01")
        } else {
            false
        }
    });
    assert!(has_defined);
}

#[test]
fn test_pos_after_parse() {
    let src = b"42+1,rest";
    let mut pos = 0;
    parse_expr(src, &mut pos).unwrap();
    // ',' で式が終了しているはず
    assert_eq!(pos, 4); // "42+1"
}

// =================================================================
// eval.rs テスト群 (式評価・セクション値計算)
// =================================================================

fn no_lookup(_: &[u8]) -> Option<EvalValue> {
    None
}

fn sym_lookup(name: &[u8]) -> Option<EvalValue> {
    if name == b"X" {
        Some(EvalValue {
            value: 0x1000,
            section: 1,
        })
    } else if name == b"Y" {
        Some(EvalValue {
            value: 0x2000,
            section: 1,
        })
    } else if name == b"C" {
        Some(EvalValue::constant(42))
    } else {
        None
    }
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
    let rpn = vec![
        T::ValueByte(1),
        T::ValueByte(2),
        T::Op(Operator::Add),
        T::End,
    ];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(3)));
}

#[test]
fn test_mul_constants() {
    // 3 * 4 = 12
    let rpn = vec![
        T::ValueByte(3),
        T::ValueByte(4),
        T::Op(Operator::Mul),
        T::End,
    ];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(12)));
}

#[test]
fn test_unary_neg_eval() {
    // -(5) = -5
    let rpn = vec![T::ValueByte(5), T::Op(Operator::Neg), T::End];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(-5)));
}

#[test]
fn test_unary_not_eval() {
    // ~1 = -2 (0xFFFFFFFE として)
    let rpn = vec![T::ValueByte(1), T::Op(Operator::Not), T::End];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(-2)));
}

#[test]
fn test_high_low_eval() {
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
    let rpn = vec![
        T::ValueByte(1),
        T::ValueByte(0),
        T::Op(Operator::Div),
        T::End,
    ];
    assert_eq!(eval(rpn), Err(EvalError::DivisionByZero));
}

#[test]
fn test_comparison_eval() {
    // 3 < 5 = -1 (true)
    let rpn = vec![
        T::ValueByte(3),
        T::ValueByte(5),
        T::Op(Operator::Slt),
        T::End,
    ];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(-1)));
    // 5 < 3 = 0 (false)
    let rpn = vec![
        T::ValueByte(5),
        T::ValueByte(3),
        T::Op(Operator::Slt),
        T::End,
    ];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(0)));
}

#[test]
fn test_location() {
    // '*' = 0x100（loc）
    let rpn = vec![T::Location, T::End];
    let res = eval_rpn(&rpn, 0x100, 0x200, 1, &no_lookup);
    assert_eq!(
        res,
        Ok(EvalValue {
            value: 0x100,
            section: 1
        })
    );
    // '$' = 0x200（cur_loc）
    let rpn = vec![T::CurrentLoc, T::End];
    let res = eval_rpn(&rpn, 0x100, 0x200, 1, &no_lookup);
    assert_eq!(
        res,
        Ok(EvalValue {
            value: 0x200,
            section: 1
        })
    );
}

#[test]
fn test_symbol_lookup() {
    let rpn = vec![
        T::SymbolRef(b"C".to_vec()),
        T::ValueByte(10),
        T::Op(Operator::Add),
        T::End,
    ];
    let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
    assert_eq!(res, Ok(EvalValue::constant(52)));
}

#[test]
fn test_addr_plus_const() {
    // X + 4 → section=1, value=0x1004
    let rpn = vec![
        T::SymbolRef(b"X".to_vec()),
        T::ValueByte(4),
        T::Op(Operator::Add),
        T::End,
    ];
    let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
    assert_eq!(
        res,
        Ok(EvalValue {
            value: 0x1004,
            section: 1
        })
    );
}

#[test]
fn test_addr_minus_addr_same_section() {
    // X - Y → 0x1000 - 0x2000 = -0x1000 (constant, section=0)
    let rpn = vec![
        T::SymbolRef(b"X".to_vec()),
        T::SymbolRef(b"Y".to_vec()),
        T::Op(Operator::Sub),
        T::End,
    ];
    let res = eval_rpn(&rpn, 0, 0, 0, &sym_lookup);
    assert_eq!(
        res,
        Ok(EvalValue {
            value: -0x1000,
            section: 0
        })
    );
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
        T::ValueByte(3),
        T::ValueByte(4),
        T::Op(Operator::Add),
        T::ValueByte(2),
        T::Op(Operator::Mul),
        T::End,
    ];
    assert_eq!(eval(rpn), Ok(EvalValue::constant(14)));
}

// =================================================================
// rpn.rs テスト群 (RPN トークン・演算子定義)
// =================================================================

#[test]
fn test_operator_priority() {
    assert!(Operator::Mul.priority() < Operator::Add.priority());
    assert!(Operator::Add.priority() < Operator::Eq.priority());
    assert!(Operator::Eq.priority() < Operator::And.priority());
    assert!(Operator::And.priority() < Operator::Or.priority());
}

#[test]
fn test_operator_is_unary() {
    assert!(Operator::Neg.is_unary());
    assert!(Operator::Not.is_unary());
    assert!(!Operator::Mul.is_unary());
    assert!(!Operator::Add.is_unary());
}

#[test]
fn test_from_u8() {
    assert_eq!(Operator::from_u8(0x01), Some(Operator::Neg));
    assert_eq!(Operator::from_u8(0x10), Some(Operator::Add));
    assert_eq!(Operator::from_u8(0x1D), Some(Operator::Or));
    assert_eq!(Operator::from_u8(0x00), None);
    assert_eq!(Operator::from_u8(0xFF), None);
}
