//! 実効アドレス解析
//!
//! 68000 基本12モードを実装（Phase 4）。
//! 68020+ 拡張モード（フルフォーマット、メモリ間接）は Phase 9 で追加予定。

pub mod encode;
pub mod parse;

pub use parse::{parse_ea, parse_reg_list_mask};

use crate::expr::{parse_expr, ParseError as ExprParseError, RPNToken, Rpn};
use crate::symbol::types::reg;
use crate::symbol::{Symbol, SymbolTable};

// ----------------------------------------------------------------
// EA モードコード定数（EAC_*、6ビットEAフィールド値）
// ----------------------------------------------------------------

/// EA モードコード定数（eamode.equ: EAC_* に対応）
pub mod eac {
    /// データレジスタ直接  Dn (000rrr)
    pub const DN: u8 = 0b000_000;
    /// アドレスレジスタ直接 An (001rrr)
    pub const AN: u8 = 0b001_000;
    /// アドレスレジスタ間接 (An) (010rrr)
    pub const ADR: u8 = 0b010_000;
    /// ポストインクリメント (An)+ (011rrr)
    pub const INCADR: u8 = 0b011_000;
    /// プリデクリメント -(An) (100rrr)
    pub const DECADR: u8 = 0b100_000;
    /// ディスプレースメント付きアドレスレジスタ間接 (d16,An) (101rrr)
    pub const DSPADR: u8 = 0b101_000;
    /// インデックス付きアドレスレジスタ間接 (d8,An,Rn) (110rrr)
    pub const IDXADR: u8 = 0b110_000;
    /// 絶対ショート xxx.w (111_000 = 0o70 = 0x38)
    pub const ABSW: u8 = 0b111_000;
    /// 絶対ロング xxx.l (111_001 = 0o71 = 0x39)
    pub const ABSL: u8 = 0b111_001;
    /// PC相対ディスプレースメント (d16,PC) (111_010 = 0o72 = 0x3A)
    pub const DSPPC: u8 = 0b111_010;
    /// PC相対インデックス (d8,PC,Rn) (111_011 = 0o73 = 0x3B)
    pub const IDXPC: u8 = 0b111_011;
    /// イミディエイト #imm (111_100 = 0o74 = 0x3C)
    pub const IMM: u8 = 0b111_100;
}

// ----------------------------------------------------------------
// EA モードビットマスク
// ----------------------------------------------------------------

/// EA モードビットマスク（eamode.equ: EA_* に対応）
pub mod ea {
    pub const DN: u16 = 1 << 0;
    pub const AN: u16 = 1 << 1;
    pub const ADR: u16 = 1 << 2;
    pub const INCADR: u16 = 1 << 3;
    pub const DECADR: u16 = 1 << 4;
    pub const DSPADR: u16 = 1 << 5;
    pub const IDXADR: u16 = 1 << 6;
    pub const ABSW: u16 = 1 << 7;
    pub const ABSL: u16 = 1 << 8;
    pub const DSPPC: u16 = 1 << 9;
    pub const IDXPC: u16 = 1 << 10;
    pub const IMM: u16 = 1 << 11;
    /// データモード（An と #imm 以外の全モード）
    pub const DATA: u16 =
        DN | ADR | INCADR | DECADR | DSPADR | IDXADR | ABSW | ABSL | DSPPC | IDXPC | IMM;
    /// メモリモード（Dn/An 以外）
    pub const MEM: u16 =
        ADR | INCADR | DECADR | DSPADR | IDXADR | ABSW | ABSL | DSPPC | IDXPC | IMM;
    /// 変更可能モード（PC相対 / #imm 以外）
    pub const ALT: u16 = DN | AN | ADR | INCADR | DECADR | DSPADR | IDXADR | ABSW | ABSL;
    /// 制御モード
    pub const CTRL: u16 = ADR | DSPADR | IDXADR | ABSW | ABSL | DSPPC | IDXPC;
    /// 全モード
    pub const ALL: u16 =
        DN | AN | ADR | INCADR | DECADR | DSPADR | IDXADR | ABSW | ABSL | DSPPC | IDXPC | IMM;
}

// ----------------------------------------------------------------
// 型定義
// ----------------------------------------------------------------

/// ディスプレースメントのサイズ指定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispSize {
    Short, // .s（brief format インデックスディスプレースメント、8ビット）
    Word,  // .w（16ビット）
    Long,  // .l（32ビット）
}

/// ディスプレースメント式
#[derive(Debug, Clone)]
pub struct Displacement {
    /// RPN式（空の Vec = ゼロディスプレースメント）
    pub rpn: Rpn,
    /// サイズ指定（None = 自動）
    pub size: Option<DispSize>,
    /// 定数値（解析時に評価できた場合）
    pub const_val: Option<i32>,
}

impl Displacement {
    /// ゼロディスプレースメント
    pub fn zero() -> Self {
        Displacement {
            rpn: vec![],
            size: None,
            const_val: Some(0),
        }
    }

    /// 定数かどうか
    pub fn is_const(&self) -> bool {
        self.const_val.is_some()
    }

    /// ゼロかどうか（定数かつ値が0）
    pub fn is_zero(&self) -> bool {
        self.const_val == Some(0)
    }
}

/// インデックスレジスタのワード/ロング指定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdxSize {
    Word, // .w（デフォルト）
    Long, // .l
}

/// スケールファクタ（68000 では *1 のみ有効、68020+ では *2/*4/*8 も可）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    S1 = 0,
    S2 = 1,
    S4 = 2,
    S8 = 3,
}

/// インデックスレジスタ指定
#[derive(Debug, Clone, Copy)]
pub struct IndexSpec {
    /// 0-7: Dn、8-15: An
    pub reg: u8,
    pub size: IdxSize,
    pub scale: Scale,
    /// レジスタサプレス（ZDn/ZAn、68020+ のみ）
    pub suppress: bool,
}

/// 実効アドレス
#[derive(Debug, Clone)]
pub enum EffectiveAddress {
    /// データレジスタ直接 Dn（n: 0-7）
    DataReg(u8),
    /// アドレスレジスタ直接 An（n: 0-7）
    AddrReg(u8),
    /// アドレスレジスタ間接 (An)
    AddrRegInd(u8),
    /// ポストインクリメント (An)+
    AddrRegPostInc(u8),
    /// プリデクリメント -(An)
    AddrRegPreDec(u8),
    /// ディスプレースメント付きアドレスレジスタ間接 (d16,An) / d16(An)
    AddrRegDisp { an: u8, disp: Displacement },
    /// インデックス付きアドレスレジスタ間接 (d8,An,Rn) / d8(An,Rn)
    AddrRegIdx {
        an: u8,
        disp: Displacement,
        idx: IndexSpec,
    },
    /// 絶対ショートアドレス xxx.w
    AbsShort(Rpn),
    /// 絶対ロングアドレス xxx.l / xxx（デフォルト）
    AbsLong(Rpn),
    /// PC相対ディスプレースメント (d16,PC)
    PcDisp(Displacement),
    /// PC相対インデックス (d8,PC,Rn)
    PcIdx { disp: Displacement, idx: IndexSpec },
    /// メモリ間接ポストインデックス ([bd,An],Xn,od) (68020+)
    MemIndPost {
        an: u8,
        bd: Displacement,
        idx: IndexSpec,
        od: Displacement,
    },
    /// メモリ間接プリインデックス ([bd,An,Xn],od) (68020+)
    MemIndPre {
        an: u8,
        bd: Displacement,
        idx: IndexSpec,
        od: Displacement,
    },
    /// PC相対メモリ間接ポストインデックス ([bd,PC],Xn,od) (68020+)
    PcMemIndPost {
        bd: Displacement,
        idx: IndexSpec,
        od: Displacement,
    },
    /// PC相対メモリ間接プリインデックス ([bd,PC,Xn],od) (68020+)
    PcMemIndPre {
        bd: Displacement,
        idx: IndexSpec,
        od: Displacement,
    },
    /// イミディエイト #imm
    Immediate(Rpn),
    /// CCR (Condition Code Register) - MOVE to/from CCR, ORI/ANDI/EORI #imm,CCR
    CcrReg,
    /// SR (Status Register) - MOVE to/from SR, ORI/ANDI/EORI #imm,SR
    SrReg,
    /// 浮動小数点データレジスタ直接 FPn（n: 0-7）
    FpReg(u8),
    /// 浮動小数点制御レジスタ（FPCR/FPSR/FPIAR）
    FpCtrlReg(u8),
}

impl EffectiveAddress {
    /// EA ビットマスクを返す
    pub fn ea_bits(&self) -> u16 {
        match self {
            Self::DataReg(_) => ea::DN,
            Self::AddrReg(_) => ea::AN,
            Self::AddrRegInd(_) => ea::ADR,
            Self::AddrRegPostInc(_) => ea::INCADR,
            Self::AddrRegPreDec(_) => ea::DECADR,
            Self::AddrRegDisp { .. } => ea::DSPADR,
            Self::AddrRegIdx { .. } => ea::IDXADR,
            Self::AbsShort(_) => ea::ABSW,
            Self::AbsLong(_) => ea::ABSL,
            Self::PcDisp(_) => ea::DSPPC,
            Self::PcIdx { .. } => ea::IDXPC,
            Self::MemIndPost { .. } => ea::IDXADR,
            Self::MemIndPre { .. } => ea::IDXADR,
            Self::PcMemIndPost { .. } => ea::IDXPC,
            Self::PcMemIndPre { .. } => ea::IDXPC,
            Self::Immediate(_) => ea::IMM,
            Self::CcrReg => 0,
            Self::SrReg => 0,
            Self::FpReg(_) => 0,
            Self::FpCtrlReg(_) => 0,
        }
    }
}

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

/// EA パースエラー
#[derive(Debug, Clone, PartialEq)]
pub enum EaError {
    /// オペランドが見つからない
    ExpectedOperand,
    /// ')' が必要
    ExpectedCloseParen,
    /// ',' が必要
    ExpectedComma,
    /// レジスタが必要
    ExpectedRegister,
    /// レジスタが不正（An が必要な位置に Dn 等）
    InvalidRegister,
    /// 不正なサイズ指定
    InvalidSize,
    /// 不正なスケール値（1/2/4/8 以外）
    InvalidScale,
    /// 不正なインデックスレジスタ（Dn/An のみ）
    InvalidIndexReg,
    /// 予期しないトークン
    UnexpectedToken,
    /// 式解析エラー
    ExprError(ExprParseError),
}

impl From<ExprParseError> for EaError {
    fn from(e: ExprParseError) -> Self {
        EaError::ExprError(e)
    }
}

// PartialEq のための実装（テスト用）
impl PartialEq for EffectiveAddress {
    fn eq(&self, other: &Self) -> bool {
        use EffectiveAddress::*;
        match (self, other) {
            (DataReg(a), DataReg(b)) => a == b,
            (AddrReg(a), AddrReg(b)) => a == b,
            (AddrRegInd(a), AddrRegInd(b)) => a == b,
            (AddrRegPostInc(a), AddrRegPostInc(b)) => a == b,
            (AddrRegPreDec(a), AddrRegPreDec(b)) => a == b,
            (AddrRegDisp { an: a, .. }, AddrRegDisp { an: b, .. }) => a == b,
            (AddrRegIdx { an: a, .. }, AddrRegIdx { an: b, .. }) => a == b,
            (AbsShort(_), AbsShort(_)) => true,
            (AbsLong(_), AbsLong(_)) => true,
            (PcDisp(_), PcDisp(_)) => true,
            (PcIdx { .. }, PcIdx { .. }) => true,
            (Immediate(_), Immediate(_)) => true,
            _ => false,
        }
    }
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::cpu;
    use crate::symbol::SymbolTable;

    fn make_sym() -> SymbolTable {
        SymbolTable::new(false)
    }

    fn parse(s: &str) -> EffectiveAddress {
        let sym = make_sym();
        let mut pos = 0;
        parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect(s)
    }

    fn parse_err(s: &str) -> EaError {
        let sym = make_sym();
        let mut pos = 0;
        parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect_err(s)
    }

    // ---- レジスタ直接 ----

    #[test]
    fn test_data_reg() {
        for i in 0..8u8 {
            let s = format!("d{}", i);
            assert_eq!(parse(&s), EffectiveAddress::DataReg(i), "{}", s);
        }
        assert_eq!(parse("D3"), EffectiveAddress::DataReg(3));
    }

    #[test]
    fn test_addr_reg() {
        for i in 0..8u8 {
            let s = format!("a{}", i);
            assert_eq!(parse(&s), EffectiveAddress::AddrReg(i), "{}", s);
        }
        assert_eq!(parse("sp"), EffectiveAddress::AddrReg(7));
        assert_eq!(parse("SP"), EffectiveAddress::AddrReg(7));
    }

    // ---- アドレスレジスタ間接 ----

    #[test]
    fn test_addr_reg_ind() {
        assert_eq!(parse("(a0)"), EffectiveAddress::AddrRegInd(0));
        assert_eq!(parse("(a5)"), EffectiveAddress::AddrRegInd(5));
        assert_eq!(parse("( a0 )"), EffectiveAddress::AddrRegInd(0));
    }

    #[test]
    fn test_post_inc() {
        assert_eq!(parse("(a0)+"), EffectiveAddress::AddrRegPostInc(0));
        assert_eq!(parse("(a7)+"), EffectiveAddress::AddrRegPostInc(7));
    }

    #[test]
    fn test_pre_dec() {
        assert_eq!(parse("-(a0)"), EffectiveAddress::AddrRegPreDec(0));
        assert_eq!(parse("-(sp)"), EffectiveAddress::AddrRegPreDec(7));
    }

    // ---- ディスプレースメント付きアドレスレジスタ間接 ----

    fn disp_val(ea: &EffectiveAddress) -> i32 {
        match ea {
            EffectiveAddress::AddrRegDisp { disp, .. } => {
                // RPN を評価して定数を得る
                crate::expr::eval_rpn(&disp.rpn, 0, 0, 0, &|_| None)
                    .unwrap()
                    .value
            }
            _ => panic!("not AddrRegDisp"),
        }
    }

    fn disp_an(ea: &EffectiveAddress) -> u8 {
        match ea {
            EffectiveAddress::AddrRegDisp { an, .. } => *an,
            _ => panic!("not AddrRegDisp"),
        }
    }

    #[test]
    fn test_addr_reg_disp() {
        // 括弧内形式 (d,An)
        let ea = parse("(4,a0)");
        assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 0, .. }));
        assert_eq!(disp_val(&ea), 4);

        // 前置形式 d(An)
        let ea2 = parse("4(a0)");
        assert!(matches!(ea2, EffectiveAddress::AddrRegDisp { an: 0, .. }));
        assert_eq!(disp_val(&ea2), 4);

        // 負のディスプレースメント
        let ea3 = parse("(-8,a5)");
        assert_eq!(disp_an(&ea3), 5);
        assert_eq!(disp_val(&ea3), -8);
    }

    #[test]
    fn test_addr_reg_disp_zero() {
        // (0,An) は (An) と同じではなく AddrRegDisp として解析される
        let ea = parse("(0,a3)");
        assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 3, .. }));
    }

    // ---- インデックス付きアドレスレジスタ間接 ----

    #[test]
    fn test_addr_reg_idx_basic() {
        // (0,a0,d1) → AddrRegIdx
        let ea = parse("(0,a0,d1)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 0);
                assert_eq!(idx.reg, 1);
                assert_eq!(idx.size, IdxSize::Word);
                assert_eq!(idx.scale, Scale::S1);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_long() {
        let ea = parse("(2,a3,d4.l)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 3);
                assert_eq!(idx.reg, 4);
                assert_eq!(idx.size, IdxSize::Long);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_an_index() {
        // インデックスレジスタに An を使う
        let ea = parse("(0,a0,a1.w)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 0);
                assert_eq!(idx.reg, 0x08 + 1); // A1
                assert_eq!(idx.size, IdxSize::Word);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_no_disp() {
        // (a0,d1) → AddrRegIdx with zero displacement
        let ea = parse("(a0,d1)");
        match &ea {
            EffectiveAddress::AddrRegIdx { an, disp, idx } => {
                assert_eq!(*an, 0);
                assert!(disp.is_zero());
                assert_eq!(idx.reg, 1);
            }
            _ => panic!("expected AddrRegIdx, got {:?}", ea),
        }
    }

    #[test]
    fn test_addr_reg_idx_dn_first() {
        // (d1,a0) → AddrRegIdx (Dn が先でも An がベース)
        let ea = parse("(d1,a0)");
        match &ea {
            EffectiveAddress::AddrRegIdx { an, disp, idx } => {
                assert_eq!(*an, 0);
                assert!(disp.is_zero());
                assert_eq!(idx.reg, 1);
            }
            _ => panic!("expected AddrRegIdx, got {:?}", ea),
        }
    }

    // ---- 絶対アドレス ----

    #[test]
    fn test_abs_short() {
        let ea = parse("$1234.w");
        assert!(matches!(ea, EffectiveAddress::AbsShort(_)));

        let ea2 = parse("($1234).w");
        assert!(matches!(ea2, EffectiveAddress::AbsShort(_)));
    }

    #[test]
    fn test_abs_long() {
        let ea = parse("$12345678.l");
        assert!(matches!(ea, EffectiveAddress::AbsLong(_)));

        // デフォルト（サイズ指定なし）はロング
        let ea2 = parse("$1000");
        assert!(matches!(ea2, EffectiveAddress::AbsLong(_)));

        let ea3 = parse("($1234).l");
        assert!(matches!(ea3, EffectiveAddress::AbsLong(_)));
    }

    // ---- PC相対 ----

    #[test]
    fn test_pc_disp() {
        let ea = parse("(4,pc)");
        assert!(matches!(ea, EffectiveAddress::PcDisp(_)));
    }

    #[test]
    fn test_pc_idx() {
        let ea = parse("(2,pc,d0)");
        assert!(matches!(ea, EffectiveAddress::PcIdx { .. }));
    }

    // ---- イミディエイト ----

    #[test]
    fn test_immediate() {
        let ea = parse("#100");
        assert!(matches!(ea, EffectiveAddress::Immediate(_)));
    }

    #[test]
    fn test_immediate_hex() {
        let ea = parse("#$FFFF");
        assert!(matches!(ea, EffectiveAddress::Immediate(_)));
    }

    // ---- EA ビットマスク ----

    #[test]
    fn test_ea_bits() {
        assert_eq!(parse("d0").ea_bits(), ea::DN);
        assert_eq!(parse("a0").ea_bits(), ea::AN);
        assert_eq!(parse("(a0)").ea_bits(), ea::ADR);
        assert_eq!(parse("(a0)+").ea_bits(), ea::INCADR);
        assert_eq!(parse("-(a0)").ea_bits(), ea::DECADR);
        assert_eq!(parse("(4,a0)").ea_bits(), ea::DSPADR);
        assert_eq!(parse("(0,a0,d0)").ea_bits(), ea::IDXADR);
        assert_eq!(parse("$1000.w").ea_bits(), ea::ABSW);
        assert_eq!(parse("$1000").ea_bits(), ea::ABSL);
        assert_eq!(parse("(4,pc)").ea_bits(), ea::DSPPC);
        assert_eq!(parse("(0,pc,d0)").ea_bits(), ea::IDXPC);
        assert_eq!(parse("#0").ea_bits(), ea::IMM);
    }

    // ---- エラーケース ----

    #[test]
    fn test_error_empty() {
        assert_eq!(parse_err(""), EaError::ExpectedOperand);
    }
}
