/// 逆ポーランド式のトークン型
///
/// オリジナルの RPN_* コード（tmpcode.equ）と演算子コード（has.equ: OP_NEG〜OP_OR）
/// に対応する。Rust版はワードコード列ではなく enum で表現する。

// ----------------------------------------------------------------
// 演算子
// ----------------------------------------------------------------

/// 演算子コード（has.equ: OP_NEG〜OP_OR）
///
/// 値は元の OP_* 定数と一致させる（シリアライズ時に使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Operator {
    // ---- 単項演算子 (OP_NEG=1 〜 OP_NUL=8) ----
    Neg   = 0x01,  // -（単項マイナス）
    Pos   = 0x02,  // +（単項プラス）
    Not   = 0x03,  // .not.（ビット反転）
    High  = 0x04,  // .high.（上位バイト）
    Low   = 0x05,  // .low. （下位バイト）
    HighW = 0x06,  // .highw.（上位ワード）
    LowW  = 0x07,  // .loww. （下位ワード）
    Nul   = 0x08,  // .nul.  （0にする）
    // ---- 二項演算子 (OP_MUL=9 〜 OP_OR=29) ----
    Mul   = 0x09,  // *
    Div   = 0x0A,  // /
    Mod   = 0x0B,  // .mod.
    Shr   = 0x0C,  // >> / .shr.
    Shl   = 0x0D,  // << / .shl.
    Asr   = 0x0E,  // .asr.（算術右シフト）
    Sub   = 0x0F,  // -
    Add   = 0x10,  // +
    Eq    = 0x11,  // = / == / .eq.
    Ne    = 0x12,  // <> / != / .ne.
    Lt    = 0x13,  // < / .lt.（符号なし）
    Le    = 0x14,  // <= / .le.（符号なし）
    Gt    = 0x15,  // > / .gt.（符号なし）
    Ge    = 0x16,  // >= / .ge.（符号なし）
    Slt   = 0x17,  // .slt.（符号付き <）
    Sle   = 0x18,  // .sle.（符号付き <=）
    Sgt   = 0x19,  // .sgt.（符号付き >）
    Sge   = 0x1A,  // .sge.（符号付き >=）
    And   = 0x1B,  // & / .and.
    Xor   = 0x1C,  // ^ / .xor.
    Or    = 0x1D,  // | / .or.
}

impl Operator {
    /// 演算子の優先順位（オリジナルの oprprior_tbl に対応）
    ///
    /// 値が小さいほど高い優先順位（先に結合する）。
    /// シャンティングヤードでは「スタック top の優先順位 <= 現在の優先順位」のとき pop。
    pub fn priority(self) -> u8 {
        match self {
            // 1: 単項演算子
            Operator::Neg | Operator::Pos | Operator::Not
            | Operator::High | Operator::Low | Operator::HighW
            | Operator::LowW | Operator::Nul => 1,
            // 2: 乗除算・シフト
            Operator::Mul | Operator::Div | Operator::Mod
            | Operator::Shr | Operator::Shl | Operator::Asr => 2,
            // 3: 加減算
            Operator::Sub | Operator::Add => 3,
            // 4: 比較演算子
            Operator::Eq | Operator::Ne | Operator::Lt | Operator::Le
            | Operator::Gt | Operator::Ge | Operator::Slt | Operator::Sle
            | Operator::Sgt | Operator::Sge => 4,
            // 5: 論理 AND
            Operator::And => 5,
            // 6: 論理 XOR / OR
            Operator::Xor | Operator::Or => 6,
        }
    }

    /// 単項演算子かどうか（OP_NEG〜OP_NUL）
    pub fn is_unary(self) -> bool {
        (self as u8) < Operator::Mul as u8
    }

    /// u8 コードから Operator に変換する
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x01 => Some(Operator::Neg),  0x02 => Some(Operator::Pos),
            0x03 => Some(Operator::Not),  0x04 => Some(Operator::High),
            0x05 => Some(Operator::Low),  0x06 => Some(Operator::HighW),
            0x07 => Some(Operator::LowW), 0x08 => Some(Operator::Nul),
            0x09 => Some(Operator::Mul),  0x0A => Some(Operator::Div),
            0x0B => Some(Operator::Mod),  0x0C => Some(Operator::Shr),
            0x0D => Some(Operator::Shl),  0x0E => Some(Operator::Asr),
            0x0F => Some(Operator::Sub),  0x10 => Some(Operator::Add),
            0x11 => Some(Operator::Eq),   0x12 => Some(Operator::Ne),
            0x13 => Some(Operator::Lt),   0x14 => Some(Operator::Le),
            0x15 => Some(Operator::Gt),   0x16 => Some(Operator::Ge),
            0x17 => Some(Operator::Slt),  0x18 => Some(Operator::Sle),
            0x19 => Some(Operator::Sgt),  0x1A => Some(Operator::Sge),
            0x1B => Some(Operator::And),  0x1C => Some(Operator::Xor),
            0x1D => Some(Operator::Or),
            _ => None,
        }
    }
}

// ----------------------------------------------------------------
// RPN トークン
// ----------------------------------------------------------------

/// 逆ポーランド式のトークン（RPN_* コードに対応）
///
/// オリジナルはワードコード列だが、Rust版は enum を使う。
/// シンボル参照はポインタではなくシンボル名バイト列で保持する。
#[derive(Debug, Clone)]
pub enum RPNToken {
    /// 8bit 即値（RPN_VALUEB = $0100）
    ValueByte(u8),
    /// 16bit 即値（RPN_VALUEW = $0200）
    ValueWord(u16),
    /// 32bit 即値（RPN_VALUE = $0300）
    Value(u32),
    /// シンボル参照（RPN_SYMBOL = $0400）
    /// オリジナルはポインタだが、Rust版はシンボル名バイト列を格納する。
    SymbolRef(Vec<u8>),
    /// 行頭ロケーションカウンタ（'*'、RPN_LOCATION = $0500）
    Location,
    /// 現在のロケーションカウンタ（'$'、RPN_LOCATION | 1 = $0501）
    CurrentLoc,
    /// 演算子（RPN_OPERATOR = $0600）
    Op(Operator),
    /// 終端（RPN_END = $0000）
    End,
}

/// 逆ポーランド式（RPNトークン列）
pub type Rpn = Vec<RPNToken>;

#[cfg(test)]
mod tests {
    use super::*;

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
}
