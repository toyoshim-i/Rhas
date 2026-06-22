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
#[allow(dead_code)]
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
mod tests;
