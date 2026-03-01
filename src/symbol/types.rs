#![allow(dead_code)]
//! シンボルテーブル型定義
//!
//! オリジナルの `symbol.equ` の構造体定義に対応する。

#[cfg(test)]
use crate::options::cpu;

// ----------------------------------------------------------------
// サイズ
// ----------------------------------------------------------------

/// 命令サイズコード（register.equ: SZ_BYTE等）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i8)]
pub enum SizeCode {
    None   = -1,
    Byte   = 0,
    Word   = 1,
    Long   = 2,
    Short  = 3,  // .s / Single(FPP)
    Double = 4,  // .d (FPP)
    Extend = 5,  // .x (FPP)
    Packed = 6,  // .p (FPP)
    Quad   = 7,  // .q (MMU)
}

/// 命令に使用できるサイズのビットセット（register.equ: SZB/SZW/SZL等）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeFlags(pub u8);

pub mod sz {
    use super::SizeFlags;
    pub const NONE: SizeFlags = SizeFlags(0x00);
    pub const B: SizeFlags    = SizeFlags(1 << 0);
    pub const W: SizeFlags    = SizeFlags(1 << 1);
    pub const L: SizeFlags    = SizeFlags(1 << 2);
    pub const S: SizeFlags    = SizeFlags(1 << 3);  // Short / Single
    pub const D: SizeFlags    = SizeFlags(1 << 4);  // Double
    pub const X: SizeFlags    = SizeFlags(1 << 5);  // Extend
    pub const P: SizeFlags    = SizeFlags(1 << 6);  // Packed
    pub const Q: SizeFlags    = SizeFlags(1 << 7);  // Quad
    pub const BW: SizeFlags   = SizeFlags(B.0 | W.0);
    pub const BWL: SizeFlags  = SizeFlags(B.0 | W.0 | L.0);
    pub const WL: SizeFlags   = SizeFlags(W.0 | L.0);
    pub const BWLS: SizeFlags = SizeFlags(B.0 | W.0 | L.0 | S.0);
}

impl SizeFlags {
    pub fn contains(self, other: SizeFlags) -> bool {
        (self.0 & other.0) != 0
    }
}

// ----------------------------------------------------------------
// CPUタイプ（cputype.equ）
// ----------------------------------------------------------------

/// CPU タイプビットマスク（オリジナルの SYM_ARCH 1ワードに対応）
///
/// 上位バイト = arch (C000〜CFPP), 下位バイト = arch2 (C520/C530/C540)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuMask(pub u16);

pub mod cmask {
    use super::CpuMask;
    use crate::options::cpu as c;
    pub const NONE: CpuMask = CpuMask(0);
    // 全 68k
    pub const M68K: CpuMask = CpuMask(c::C000 | c::C010 | c::C020 | c::C030 | c::C040 | c::C060);
    // 全 ColdFire
    pub const CF: CpuMask   = CpuMask(c::C520 | c::C530 | c::C540);
    // 全 CPU（疑似命令含む）
    pub const ALL: CpuMask  = CpuMask(
        c::C000|c::C010|c::C020|c::C030|c::C040|c::C060|c::CMMU|c::CFPP|c::C520|c::C530|c::C540
    );
    // 68000〜68060 + 全 CF
    pub const ALL_INSN: CpuMask = CpuMask(
        c::C000|c::C010|c::C020|c::C030|c::C040|c::C060|c::C520|c::C530|c::C540
    );
    // 68020以降 + CF
    pub const C020UP: CpuMask = CpuMask(
        c::C020|c::C030|c::C040|c::C060|c::C520|c::C530|c::C540
    );
}

impl CpuMask {
    /// 疑似命令かどうか（arch_word == 0）
    pub fn is_pseudo(self) -> bool {
        self.0 == 0
    }
    /// 現在のCPUタイプと一致するか
    pub fn matches(self, current: u16) -> bool {
        self.is_pseudo() || (self.0 & current) != 0
    }
}

// ----------------------------------------------------------------
// レジスタコード（register.equ）
// ----------------------------------------------------------------

/// レジスタコード定数（REG_D0〜REG_FPIAR）
pub mod reg {
    // データレジスタ
    pub const D0: u8 = 0x00;
    pub const D1: u8 = 0x01;
    pub const D2: u8 = 0x02;
    pub const D3: u8 = 0x03;
    pub const D4: u8 = 0x04;
    pub const D5: u8 = 0x05;
    pub const D6: u8 = 0x06;
    pub const D7: u8 = 0x07;
    // アドレスレジスタ
    pub const A0: u8 = 0x08;
    pub const A1: u8 = 0x09;
    pub const A2: u8 = 0x0A;
    pub const A3: u8 = 0x0B;
    pub const A4: u8 = 0x0C;
    pub const A5: u8 = 0x0D;
    pub const A6: u8 = 0x0E;
    pub const A7: u8 = 0x0F;
    pub const SP: u8 = 0x0F;
    // サプレスレジスタ（ZDn, ZAn）
    pub const ZD0: u8 = 0x10;
    pub const ZD7: u8 = 0x17;
    pub const ZA0: u8 = 0x18;
    pub const ZA7: u8 = 0x1F;
    pub const ZPC: u8 = 0x2E;
    // 特殊レジスタ
    pub const PC:  u8 = 0x20;
    pub const CCR: u8 = 0x21;
    pub const SR:  u8 = 0x22;
    pub const USP: u8 = 0x23;
    pub const SFC: u8 = 0x24;
    pub const DFC: u8 = 0x25;
    pub const VBR: u8 = 0x26;
    pub const MSP: u8 = 0x27;
    pub const ISP: u8 = 0x28;
    pub const CACR: u8 = 0x29;
    pub const CAAR: u8 = 0x2A;
    pub const BUSCR: u8 = 0x2B;
    pub const PCR: u8  = 0x2C;
    pub const OPC: u8  = 0x2F;
    // MMUレジスタ
    pub const CRP: u8   = 0x40;
    pub const SRP: u8   = 0x41;
    pub const TC: u8    = 0x42;
    pub const TT0: u8   = 0x43;
    pub const TT1: u8   = 0x44;
    pub const MMUSR: u8 = 0x45;
    pub const URP: u8   = 0x46;
    pub const ITT0: u8  = 0x47;
    pub const ITT1: u8  = 0x48;
    pub const DTT0: u8  = 0x49;
    pub const DTT1: u8  = 0x4A;
    pub const NC: u8    = 0x5C;
    pub const DC: u8    = 0x5D;
    pub const IC: u8    = 0x5E;
    pub const BC: u8    = 0x5F;
    // ColdFireレジスタ
    pub const ROMBAR: u8  = 0x30;
    pub const RAMBAR0: u8 = 0x34;
    pub const RAMBAR1: u8 = 0x35;
    pub const MBAR: u8    = 0x3F;
    pub const ACC: u8     = 0x70;
    pub const MACSR: u8   = 0x74;
    pub const MASK: u8    = 0x76;
    // FPPレジスタ
    pub const FP0: u8   = 0x80;
    pub const FP1: u8   = 0x81;
    pub const FP2: u8   = 0x82;
    pub const FP3: u8   = 0x83;
    pub const FP4: u8   = 0x84;
    pub const FP5: u8   = 0x85;
    pub const FP6: u8   = 0x86;
    pub const FP7: u8   = 0x87;
    pub const FPCR: u8  = 0x88;
    pub const FPSR: u8  = 0x89;
    pub const FPIAR: u8 = 0x8A;
}

// ----------------------------------------------------------------
// 命令ハンドラ識別子（Phase 5 で実装する処理ルーチン）
// ----------------------------------------------------------------

/// 命令・疑似命令のハンドラ識別子
///
/// `opname.s` の `~handler` / `~~handler` に対応する。
/// Phase 5-9 で各ハンドラを実装する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsnHandler {
    // ---- データ転送 ----
    Move,
    MoveQ,
    MoveA,
    MoveM,
    MoveP,
    Lea,
    PeaJsrJmp,   // pea / jsr / jmp（ソース共通）
    JmpJsr,
    // ---- 算術 ----
    SubAdd,      // add / sub
    SubAddQ,     // addq / subq
    SubAddI,     // addi / subi
    SbAdCpA,     // adda / suba / cmpa（ソース共通）
    SubAddX,     // addx / subx
    DivMul,      // mulu/muls/divu/divs
    NegNot,      // neg/negx/not/nbcd
    Clr,
    Tst,
    Ext,
    Swap,
    Exg,
    Chk,
    // ---- 比較 ----
    Cmp,
    CmpI,
    CmpA,
    CmpM,
    // ---- 論理 ----
    OrAnd,       // or/and
    OrAndEorI,   // ori/andi/eori
    Eor,
    // ---- ビット操作 ----
    BchClSt,     // bchg/bclr/bset
    Btst,
    // ---- シフト / ローテート ----
    SftRot,      // asr/lsr/ror/rol/roxr/roxl
    Asl,         // asl（-b1による最適化あり）
    // ---- BCD / パック10進 ----
    SAbcd,       // sbcd/abcd
    // ---- 分岐 ----
    Bcc,
    JBcc,        // jbra/jbsr/jbhi等（長距離分岐）
    DBcc,
    Scc,
    DecInc,      // dec/inc（HAS独自）
    Link,
    Unlk,
    Trap,
    StopRtd,     // stop/rtd
    // ---- 疑似命令 ----
    Even,
    Quad,
    Align,
    Dc,
    Ds,
    Dcb,
    Equ,
    Set,
    Reg,
    Rept,
    Irp,
    Irpc,
    Xdef,
    Xref,
    Globl,
    Comm,
    Stack,
    Offset,
    OffsymPs,
    MacroDef,
    ExitM,
    EndM,
    Local,
    SizeM,
    If,
    Iff,
    Ifdef,
    Ifndef,
    Else,
    Elseif,
    Endif,
    End,
    Insert,
    Include,
    Request,
    List,
    Nlist,
    Lall,
    Sall,
    Width,
    Page,
    Title,
    SubTtl,
    Fail,
    Cpu,
    // セクション切り替え
    TextSect,
    DataSect,
    BssSect,
    RdataSect,
    RbssSect,
    RstackSect,
    RldataSect,
    RlbssSect,
    RlstackSect,
    Rcomm,
    Rlcomm,
    // SCD デバッグ（Phase 10）
    FileScd,
    Def,
    Endef,
    Val,
    Scl,
    TypeScd,
    Tag,
    Ln,
    Line,
    SizeScd,
    Dim,
    // CPU 指示
    Cpu68000,
    Cpu68010,
    Cpu68020,
    Cpu68030,
    Cpu68040,
    Cpu68060,
    Cpu5200,
    Cpu5300,
    Cpu5400,
    FpId,
    Pragma,
    // Phase 9: 68020+ / FPU / ColdFire
    ExtB,        // EXTB.L Dn
    Bkpt,        // BKPT #n
    Trapcc,      // TRAPcc / TRAPcc.W / TRAPcc.L
    BfChgClrSet, // BFCHG/BFCLR/BFSET/BFTST <ea>{offset:width}
    BfExtFfo,    // BFEXTU/BFEXTS/BFFFO <ea>{offset:width},Dn
    BfIns,       // BFINS Dn,<ea>{offset:width}
    MovesInsn,   // MOVES.sz <ea>,Rn / MOVES.sz Rn,<ea>
    MoveC,       // MOVEC Rn,CReg / MOVEC CReg,Rn
    PackUnpk,    // PACK/UNPK
    CasInsn,     // CAS Dc,Du,<ea>
    Cas2Insn,    // CAS2.W/CAS2.L Dc1:Dc2,Du1:Du2,(Rn1):(Rn2)
    DivSlUl,     // DIVSL.L/DIVUL.L <ea>,Dr:Dq
    CmpChk2,     // CMP2/CHK2 <ea>,Rn
    Move16Insn,  // MOVE16 (Ax)+,(Ay)+ 等
    CInvPushLP,  // CINVL/CINVP/CPUSHL/CPUSHP cache_set,(An)
    CInvPushA,   // CINVA/CPUSHA cache_set
    // FPU (68881/68882)
    FMove,       // FMOVE
    FMoveM,      // FMOVEM (control-register subset)
    FMoveCr,     // FMOVECR
    FSinCos,     // FSINCOS
    FArith,      // FADD/FSUB/FMUL/FDIV
    FCmp,        // FCMP
    FTst,        // FTST
    FNop,        // FNOP
    FSave,       // FSAVE
    FRestore,    // FRESTORE
    FBcc,        // FBcc
    FDBcc,       // FDBcc
}

// ----------------------------------------------------------------
// シンボル本体
// ----------------------------------------------------------------

/// シンボル定義属性（has.equ: SA_UNDEF〜SA_PREDEFINE）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum DefAttrib {
    Undef     = 0,  // SA_UNDEF: 使用されたが未定義
    NoDet     = 1,  // SA_NODET: 定義されたが値未確定
    Define    = 2,  // SA_DEFINE: 定義・値確定
    Predefine = 3,  // SA_PREDEFINE: 定義・値確定・再定義不可
}

/// 外部参照属性（has.equ: SECT_XREF〜SECT_GLOBL）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtAttrib {
    None,
    XRef,   // $FF: 外部参照
    XDef,   // $FB: 外部定義
    Globl,  // $FA: グローバル（外部参照/定義）
    Comm,   // $FE: コモンエリア
    RComm,  // $FD: コモンエリア（64KB以内相対）
    RLComm, // $FC: コモンエリア（64KB以上相対）
}

/// シンボルの最初の定義方法（ST_VALUE のみ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstDef {
    Other  = 0,
    Set    = -1,   // .set(=) で定義
    Offsym = 1,    // .offsym で定義
}

/// シンボルテーブルエントリ（symbol.equ の各 ST_* に対応）
#[derive(Debug, Clone)]
pub enum Symbol {
    /// 数値シンボル / ローカルラベル（ST_VALUE / ST_LOCAL）
    Value {
        attrib: DefAttrib,
        ext_attrib: ExtAttrib,
        section: u8,
        org_num: u8,
        first: FirstDef,
        opt_count: u8,
        value: i32,
    },

    /// 浮動小数点実数シンボル（ST_REAL）
    Real {
        size: SizeCode,
        data: Vec<u8>,
    },

    /// .reg シンボル（ST_REGSYM）
    /// define: コンマ区切りの各要素のRPN式リスト
    RegSym {
        define: Vec<Vec<crate::expr::rpn::RPNToken>>,
    },

    /// レジスタ名（ST_REGISTER）- 予約済みシンボル
    Register {
        /// CPUタイプビットマスク（SYM_ARCH<<8 | SYM_ARCH2）
        arch: CpuMask,
        /// レジスタコード（REG_D0 etc.）
        regno: u8,
    },

    /// 命令名（ST_OPCODE）- 予約済みシンボル
    Opcode {
        /// オペランドを持たない命令か（NOP/RTS等）
        noopr: bool,
        /// CPUタイプビットマスク（0 = 疑似命令）
        arch: CpuMask,
        /// 使用可能なサイズ（68k）
        size: SizeFlags,
        /// 使用可能なサイズ（ColdFire）
        size2: SizeFlags,
        /// 命令コード基本パターン（SYM_OPCODE）
        opcode: u16,
        /// 処理ルーチン識別子
        handler: InsnHandler,
    },

    /// マクロ（ST_MACRO）
    Macro {
        /// 仮引数名リスト（順序が引数番号に対応）
        params: Vec<Vec<u8>>,
        /// 定義内の @ラベル 数（ローカルラベルカウンタ）
        local_count: u16,
        /// ボディ行列（各行は \n 区切り、末尾 \n なし）
        /// @name → \xFE num_hi num_lo + name\0 に変換済み
        template: Vec<u8>,
    },
}

impl Symbol {
    /// 予約済みシンボル（レジスタ名・命令名）かどうか
    pub fn is_builtin(&self) -> bool {
        matches!(self, Symbol::Register { .. } | Symbol::Opcode { .. })
    }

    /// 疑似命令かどうか
    pub fn is_pseudo(&self) -> bool {
        matches!(self, Symbol::Opcode { arch, .. } if arch.is_pseudo())
    }

    /// このシンボルが現在の CPU で使用可能か
    pub fn is_available_for_cpu(&self, cpu_type: u16) -> bool {
        match self {
            Symbol::Register { arch, .. } => arch.matches(cpu_type),
            Symbol::Opcode { arch, .. } => arch.matches(cpu_type),
            _ => true,
        }
    }

    /// ローカルラベルかどうか（Phase 2 では使用しない）
    pub fn is_local(&self) -> bool {
        false // ST_LOCAL は後で区別する
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_flags() {
        assert!(sz::BWL.contains(sz::B));
        assert!(sz::BWL.contains(sz::W));
        assert!(sz::BWL.contains(sz::L));
        assert!(!sz::BWL.contains(sz::S));
    }

    #[test]
    fn test_cpu_mask_pseudo() {
        assert!(CpuMask(0).is_pseudo());
        assert!(!CpuMask(cpu::C000).is_pseudo());
    }

    #[test]
    fn test_cpu_mask_matches() {
        let m = CpuMask(cpu::C000 | cpu::C010);
        assert!(m.matches(cpu::C000));
        assert!(m.matches(cpu::C010));
        assert!(!m.matches(cpu::C020));
        // 疑似命令は全CPUで使用可能
        assert!(CpuMask(0).matches(cpu::C000));
        assert!(CpuMask(0).matches(cpu::C060));
    }
}
