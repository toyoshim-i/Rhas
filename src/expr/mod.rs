//! 式パーサー（ソーステキスト → 逆ポーランド式）
//!
//! オリジナルの `convrpn`（expr.s）と字句解析部分に対応する。
//! Rust版はソーステキストから直接 RPN トークン列を生成する。
//! シャンティングヤードアルゴリズムを使用する。

pub mod eval;
pub mod parse;
pub mod rpn;

#[allow(unused_imports)]
pub use eval::{eval_rpn, EvalError, EvalValue};
pub use parse::parse_expr;
#[allow(unused_imports)]
pub use rpn::{Operator, RPNToken, Rpn};

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

/// 式パーサーのエラー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// 式が見つからない（空または予期しない文字）
    ExprExpected,
    /// 閉じ括弧がない
    UnclosedParen,
    /// 閉じ括弧が多すぎる
    UnexpectedCloseParen,
    /// 文字定数が閉じていない
    UnclosedCharConst,
    /// 数値が不正
    InvalidNumber,
    /// `>>>`（算術右シフトの別記法）サポートのための内部記号
    Internal,
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
}
