#![allow(dead_code)]
//! シンボルテーブル
//!
//! オリジナルの `symbol.s`（SYMHASHPTR / CMDHASHPTR）に対応する。
//! Rust版はHashMapで実装する。
//!
//! ## テーブル構成
//! - `user_syms`: ユーザー定義シンボル（大文字小文字区別、ラベル・.equ等）
//! - `reg_table`: レジスタ名（大文字小文字区別なし、起動時登録）
//! - `cmd_table`: 命令名・マクロ名（大文字小文字区別なし）

pub mod types;

use std::collections::HashMap;
use types::{CpuMask, DefAttrib, ExtAttrib, FirstDef, InsnHandler, SizeFlags};
use types::{cmask, sz, reg};
pub use types::Symbol;

// ----------------------------------------------------------------
// SymbolTable
// ----------------------------------------------------------------

/// アセンブラのシンボルテーブル全体
pub struct SymbolTable {
    /// ユーザー定義シンボル（ラベル、.equ、.reg等）
    /// キー: 元のバイト列（大文字小文字区別）
    user_syms: HashMap<Vec<u8>, Symbol>,

    /// レジスタ名テーブル
    /// キー: 小文字化したバイト列
    reg_table: HashMap<Vec<u8>, Symbol>,

    /// 命令名・マクロ名テーブル
    /// キー: 小文字化したバイト列
    cmd_table: HashMap<Vec<u8>, Symbol>,

    /// シンボル識別長 8 バイト制限（-8 オプション）
    sym_len8: bool,
}

impl SymbolTable {
    /// 空のシンボルテーブルを作成し、予約済みシンボルを登録する
    pub fn new(sym_len8: bool) -> Self {
        let mut tbl = SymbolTable {
            user_syms: HashMap::new(),
            reg_table: HashMap::new(),
            cmd_table: HashMap::new(),
            sym_len8,
        };
        tbl.register_builtins();
        tbl
    }

    /// 予約済みシンボル（レジスタ名・命令名）を登録する
    /// オリジナルの `defressym` に対応する。
    fn register_builtins(&mut self) {
        // レジスタ名
        for (name, arch, regno) in REGISTER_TABLE {
            let key = to_lowercase_vec(name.as_bytes());
            self.reg_table.insert(
                key,
                Symbol::Register { arch: CpuMask(*arch), regno: *regno },
            );
        }
        // HAS060X cache set specifiers (dc=1, ic=2, bc=3) for CINV/CPUSH
        for (name, val) in [("dc", 1i32), ("ic", 2), ("bc", 3)] {
            self.user_syms.insert(
                name.as_bytes().to_vec(),
                Symbol::Value {
                    attrib: DefAttrib::Define,
                    ext_attrib: ExtAttrib::None,
                    section: 0,
                    org_num: 0,
                    first: FirstDef::Other,
                    opt_count: 0,
                    value: val,
                },
            );
        }
        // 命令名・疑似命令名
        for entry in OPCODE_TABLE {
            let key = to_lowercase_vec(entry.name.as_bytes());
            self.cmd_table.insert(
                key,
                Symbol::Opcode {
                    noopr:   entry.noopr,
                    arch:    entry.arch,
                    size:    entry.size,
                    size2:   entry.size2,
                    opcode:  entry.opcode,
                    handler: entry.handler,
                },
            );
        }
    }

    // ----------------------------------------------------------------
    // シンボル検索
    // ----------------------------------------------------------------

    /// シンボルを名前で検索する（ユーザー定義シンボル用）
    ///
    /// オリジナルの `isdefdsym` に対応する。
    /// - ユーザー定義シンボルは大文字小文字を区別する
    /// - ローカルラベルは検索対象から除外される
    pub fn lookup_sym(&self, name: &[u8]) -> Option<&Symbol> {
        let name = self.truncate_if_len8(name);
        self.user_syms.get(name)
    }

    /// レジスタ名を検索する（大文字小文字区別なし）
    ///
    /// CPU タイプに一致しないレジスタは返さない。
    pub fn lookup_reg(&self, name: &[u8], cpu_type: u16) -> Option<&Symbol> {
        let key = to_lowercase_vec(name);
        let sym = self.reg_table.get(&key)?;
        if sym.is_available_for_cpu(cpu_type) {
            Some(sym)
        } else {
            None
        }
    }

    /// 命令名 / マクロ名を検索する（大文字小文字区別なし）
    ///
    /// オリジナルの `isdefdmac`（マクロ）/ 命令テーブル検索に対応。
    /// CPU タイプに一致しない命令は返さない。
    pub fn lookup_cmd(&self, name: &[u8], cpu_type: u16) -> Option<&Symbol> {
        let key = to_lowercase_vec(self.truncate_if_len8(name));
        let sym = self.cmd_table.get(&key)?;
        if sym.is_available_for_cpu(cpu_type) {
            Some(sym)
        } else {
            None
        }
    }

    // ----------------------------------------------------------------
    // ユーザーシンボル登録
    // ----------------------------------------------------------------

    /// シンボルを登録する（ラベル定義、.equ 等）
    pub fn define(&mut self, name: Vec<u8>, sym: Symbol) {
        let key = self.make_user_key(name);
        self.user_syms.insert(key, sym);
    }

    /// マクロを登録する（.macro/.endm 処理後）
    pub fn define_macro(&mut self, name: Vec<u8>, sym: Symbol) {
        let key = to_lowercase_vec(name);
        self.cmd_table.insert(key, sym);
    }

    /// シンボルを名前で検索して可変参照を返す（ext_attrib 更新等に使用）
    pub fn lookup_sym_mut(&mut self, name: &[u8]) -> Option<&mut Symbol> {
        let key = if self.sym_len8 && name.len() > 8 { &name[..8] } else { name };
        self.user_syms.get_mut(key)
    }

    /// シンボルが存在するかどうか確認する
    pub fn is_defined(&self, name: &[u8]) -> bool {
        self.lookup_sym(name).is_some()
    }

    // ----------------------------------------------------------------
    // ヘルパー
    // ----------------------------------------------------------------

    /// -8 オプション時にシンボル名を 8 バイトに切り詰める
    fn truncate_if_len8<'a>(&self, name: &'a [u8]) -> &'a [u8] {
        if self.sym_len8 && name.len() > 8 {
            &name[..8]
        } else {
            name
        }
    }

    fn make_user_key(&self, name: Vec<u8>) -> Vec<u8> {
        if self.sym_len8 && name.len() > 8 {
            name[..8].to_vec()
        } else {
            name
        }
    }

    /// 統計情報
    pub fn user_sym_count(&self) -> usize {
        self.user_syms.len()
    }

    pub fn cmd_count(&self) -> usize {
        self.cmd_table.len()
    }

    pub fn reg_count(&self) -> usize {
        self.reg_table.len()
    }

    /// ユーザー定義シンボルの全エントリをイテレートする
    pub fn iter_user_syms(&self) -> impl Iterator<Item = (&Vec<u8>, &Symbol)> {
        self.user_syms.iter()
    }
}

/// バイト列を ASCII 小文字化する
fn to_lowercase_vec<B: AsRef<[u8]>>(s: B) -> Vec<u8> {
    s.as_ref().iter().map(|c| c.to_ascii_lowercase()).collect()
}

// ----------------------------------------------------------------
// レジスタ名テーブル（regname.s より）
// ----------------------------------------------------------------

/// レジスタ名テーブルエントリ: (名前, CPUタイプビットマスク, レジスタコード)
type RegEntry = (&'static str, u16, u8);

/// 全レジスタ名テーブル（regname.s の reg_tbl に対応）
///
/// フォーマット: (name, arch_word, regno)
/// arch_word = (SYM_ARCH<<8) | SYM_ARCH2 = cputype.equ の C000|C010|... の値
static REGISTER_TABLE: &[RegEntry] = &[
    // データレジスタ (D0-D7, R0-R7)
    ("d0",  0xFF01, reg::D0), ("d1",  0xFF01, reg::D1), ("d2",  0xFF01, reg::D2),
    ("d3",  0xFF01, reg::D3), ("d4",  0xFF01, reg::D4), ("d5",  0xFF01, reg::D5),
    ("d6",  0xFF01, reg::D6), ("d7",  0xFF01, reg::D7),
    // アドレスレジスタ (A0-A7, R8-R15, SP)
    ("a0",  0xFF01, reg::A0), ("a1",  0xFF01, reg::A1), ("a2",  0xFF01, reg::A2),
    ("a3",  0xFF01, reg::A3), ("a4",  0xFF01, reg::A4), ("a5",  0xFF01, reg::A5),
    ("a6",  0xFF01, reg::A6), ("a7",  0xFF01, reg::A7), ("sp",  0xFF01, reg::SP),
    // R0-R15 エイリアス
    ("r0",  0xFF01, reg::D0), ("r1",  0xFF01, reg::D1), ("r2",  0xFF01, reg::D2),
    ("r3",  0xFF01, reg::D3), ("r4",  0xFF01, reg::D4), ("r5",  0xFF01, reg::D5),
    ("r6",  0xFF01, reg::D6), ("r7",  0xFF01, reg::D7),
    ("r8",  0xFF01, reg::A0), ("r9",  0xFF01, reg::A1), ("r10", 0xFF01, reg::A2),
    ("r11", 0xFF01, reg::A3), ("r12", 0xFF01, reg::A4), ("r13", 0xFF01, reg::A5),
    ("r14", 0xFF01, reg::A6), ("r15", 0xFF01, reg::A7),
    // サプレスレジスタ（68020以降）
    ("zd0", 0x3C00, reg::ZD0), ("zd1", 0x3C00, 0x11), ("zd2", 0x3C00, 0x12),
    ("zd3", 0x3C00, 0x13),     ("zd4", 0x3C00, 0x14), ("zd5", 0x3C00, 0x15),
    ("zd6", 0x3C00, 0x16),     ("zd7", 0x3C00, reg::ZD7),
    ("za0", 0x3C00, reg::ZA0), ("za1", 0x3C00, 0x19), ("za2", 0x3C00, 0x1A),
    ("za3", 0x3C00, 0x1B),     ("za4", 0x3C00, 0x1C), ("za5", 0x3C00, 0x1D),
    ("za6", 0x3C00, 0x1E),     ("za7", 0x3C00, reg::ZA7), ("zsp", 0x3C00, reg::ZA7),
    ("zr0", 0x3C00, reg::ZD0), ("zr1", 0x3C00, 0x11), ("zr2", 0x3C00, 0x12),
    ("zr3", 0x3C00, 0x13),     ("zr4", 0x3C00, 0x14), ("zr5", 0x3C00, 0x15),
    ("zr6", 0x3C00, 0x16),     ("zr7", 0x3C00, reg::ZD7),
    ("zpc", 0x3C00, reg::ZPC),
    // PC / 制御レジスタ
    ("pc",  0xFF01, reg::PC), ("ccr", 0xFF01, reg::CCR), ("sr", 0xFF01, reg::SR),
    ("usp", 0x3F00, reg::USP),
    // 68010以降
    ("sfc",  0x3E00, reg::SFC), ("dfc",  0x3E00, reg::DFC), ("vbr", 0x3E01, reg::VBR),
    // 68020以降
    ("msp",  0x3C00, reg::MSP), ("isp",  0x3C00, reg::ISP),
    ("cacr", 0x3C07, reg::CACR), ("caar", 0x0C00, reg::CAAR),
    // 68060
    ("buscr", 0x2000, reg::BUSCR), ("pcr", 0x2000, reg::PCR),
    // FPPレジスタ（68040/68060/CFPP）
    ("fp0", 0x3080, reg::FP0), ("fp1", 0x3080, reg::FP1), ("fp2", 0x3080, reg::FP2),
    ("fp3", 0x3080, reg::FP3), ("fp4", 0x3080, reg::FP4), ("fp5", 0x3080, reg::FP5),
    ("fp6", 0x3080, reg::FP6), ("fp7", 0x3080, reg::FP7),
    ("fpcr",  0x3080, reg::FPCR), ("fpsr",  0x3080, reg::FPSR),
    ("fpiar", 0x3080, reg::FPIAR),
    // ColdFire専用
    ("rombar",  0x0007, reg::ROMBAR),  ("rambar0", 0x0007, reg::RAMBAR0),
    ("rambar1", 0x0007, reg::RAMBAR1), ("mbar",    0x0007, reg::MBAR),
    ("acc",     0x0007, reg::ACC),     ("macsr",   0x0007, reg::MACSR),
    ("mask",    0x0007, reg::MASK),
];

// ----------------------------------------------------------------
// 命令名テーブル（opname.s より）
// ----------------------------------------------------------------

/// 命令テーブルエントリ
struct OpcodeEntry {
    name:    &'static str,
    handler: InsnHandler,
    opcode:  u16,
    arch:    CpuMask,
    size:    SizeFlags,
    size2:   SizeFlags,
    noopr:   bool,
}

impl OpcodeEntry {
    const fn op(
        name: &'static str, handler: InsnHandler, opcode: u16,
        arch: CpuMask, size: SizeFlags, size2: SizeFlags,
    ) -> Self {
        OpcodeEntry { name, handler, opcode, arch, size, size2, noopr: false }
    }
    /// オペランドなし命令（NOP/RTS等）
    const fn noop(
        name: &'static str, handler: InsnHandler, opcode: u16, arch: CpuMask,
    ) -> Self {
        OpcodeEntry { name, handler, opcode, arch, size: sz::NONE, size2: sz::NONE, noopr: true }
    }
    /// 疑似命令（arch = NONE = 全CPU）
    const fn pseudo(name: &'static str, handler: InsnHandler) -> Self {
        OpcodeEntry { name, handler, opcode: 0, arch: CpuMask(0), size: sz::NONE, size2: sz::NONE, noopr: false }
    }
    /// サイズ付き疑似命令
    const fn pseudos(name: &'static str, handler: InsnHandler, size: SizeFlags) -> Self {
        OpcodeEntry { name, handler, opcode: 0, arch: CpuMask(0), size, size2: sz::NONE, noopr: false }
    }
}

/// 全CPUで使用可能（68000〜68060 + 全CF）
const ALL: CpuMask = cmask::ALL_INSN;
/// 68000〜68060のみ（ColdFire不可）
const M68K: CpuMask = cmask::M68K;
/// 68020以降
const C020_UP: CpuMask = cmask::C020UP;
/// 68010以降
const C010_UP: CpuMask = CpuMask(
    crate::options::cpu::C010 | crate::options::cpu::C020 |
    crate::options::cpu::C030 | crate::options::cpu::C040 |
    crate::options::cpu::C060
);
/// 全68k + CF530/CF540（C520を含まない - divu/divs用）
const M68K_CF53: CpuMask = CpuMask(
    crate::options::cpu::C000 | crate::options::cpu::C010 |
    crate::options::cpu::C020 | crate::options::cpu::C030 |
    crate::options::cpu::C040 | crate::options::cpu::C060 |
    crate::options::cpu::C530 | crate::options::cpu::C540
);

/// 全命令・疑似命令テーブル（opname.s の tablebody マクロ展開に対応）
static OPCODE_TABLE: &[OpcodeEntry] = &[
    // ---- データ転送 ----
    OpcodeEntry::op("move",  InsnHandler::Move,    0x0000, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("moveq", InsnHandler::MoveQ,   0x7000, ALL, sz::L,    sz::L),
    OpcodeEntry::op("movea", InsnHandler::MoveA,   0x2040, ALL, sz::WL,   sz::WL),
    OpcodeEntry::op("movem", InsnHandler::MoveM,   0x4880, ALL, sz::WL,   sz::L),
    OpcodeEntry::op("lea",   InsnHandler::Lea,     0x41C0, ALL, sz::L,    sz::L),
    OpcodeEntry::op("pea",   InsnHandler::PeaJsrJmp, 0x4840, ALL, sz::L,  sz::L),
    OpcodeEntry::noop("jsr", InsnHandler::JmpJsr,  0x4E80, ALL),
    OpcodeEntry::noop("jmp", InsnHandler::JmpJsr,  0x4EC0, ALL),
    OpcodeEntry::op("movep", InsnHandler::MoveP,   0x0108, M68K, sz::WL,  sz::NONE),
    // ---- 算術 ----
    OpcodeEntry::op("add",   InsnHandler::SubAdd,  0xD000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("addq",  InsnHandler::SubAddQ, 0x5000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("addi",  InsnHandler::SubAddI, 0x0600, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("adda",  InsnHandler::SbAdCpA, 0xD0C0, ALL, sz::WL,   sz::L),
    OpcodeEntry::op("addx",  InsnHandler::SubAddX, 0xD100, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("sub",   InsnHandler::SubAdd,  0x9000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("subq",  InsnHandler::SubAddQ, 0x5100, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("subi",  InsnHandler::SubAddI, 0x0400, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("suba",  InsnHandler::SbAdCpA, 0x90C0, ALL, sz::WL,   sz::L),
    OpcodeEntry::op("subx",  InsnHandler::SubAddX, 0x9100, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("cmp",   InsnHandler::Cmp,     0xB000, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("cmpi",  InsnHandler::CmpI,    0x0C00, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("cmpa",  InsnHandler::CmpA,    0xB0C0, ALL, sz::WL,   sz::WL),
    OpcodeEntry::op("cmpm",  InsnHandler::CmpM,    0xB108, M68K, sz::BWL, sz::NONE),
    OpcodeEntry::op("neg",   InsnHandler::NegNot,  0x4400, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("negx",  InsnHandler::NegNot,  0x4000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("clr",   InsnHandler::Clr,     0x4200, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("ext",   InsnHandler::Ext,     0x4880, ALL, sz::WL,   sz::WL),
    OpcodeEntry::op("tst",   InsnHandler::Tst,     0x4A00, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("swap",  InsnHandler::Swap,    0x4840, ALL, sz::W,    sz::W),
    OpcodeEntry::op("exg",   InsnHandler::Exg,     0xC100, M68K, sz::L,   sz::NONE),
    OpcodeEntry::op("mulu",  InsnHandler::DivMul,  0xC0C0, ALL,       sz::WL, sz::WL),
    OpcodeEntry::op("muls",  InsnHandler::DivMul,  0xC1C0, ALL,       sz::WL, sz::WL),
    OpcodeEntry::op("divu",  InsnHandler::DivMul,  0x80C0, M68K_CF53, sz::WL, sz::WL),
    OpcodeEntry::op("divs",  InsnHandler::DivMul,  0x81C0, M68K_CF53, sz::WL, sz::WL),
    OpcodeEntry::op("chk",   InsnHandler::Chk,     0x4100, M68K, sz::WL,  sz::NONE),
    OpcodeEntry::op("abcd",  InsnHandler::SAbcd,   0xC100, M68K, sz::B,   sz::NONE),
    OpcodeEntry::op("sbcd",  InsnHandler::SAbcd,   0x8100, M68K, sz::B,   sz::NONE),
    OpcodeEntry::op("nbcd",  InsnHandler::Scc,     0x4800, M68K, sz::B,   sz::NONE),
    OpcodeEntry::op("dec",   InsnHandler::DecInc,  0x5300, ALL, sz::BWL,  sz::BWL),
    OpcodeEntry::op("inc",   InsnHandler::DecInc,  0x5200, ALL, sz::BWL,  sz::BWL),
    // ---- 論理 ----
    OpcodeEntry::op("and",   InsnHandler::OrAnd,   0xC000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("andi",  InsnHandler::OrAndEorI, 0x0200, ALL, sz::BWL, sz::L),
    OpcodeEntry::op("or",    InsnHandler::OrAnd,   0x8000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("ori",   InsnHandler::OrAndEorI, 0x0000, ALL, sz::BWL, sz::L),
    OpcodeEntry::op("eor",   InsnHandler::Eor,     0xB100, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("eori",  InsnHandler::OrAndEorI, 0x0A00, ALL, sz::BWL, sz::L),
    OpcodeEntry::op("not",   InsnHandler::NegNot,  0x4600, ALL, sz::BWL,  sz::L),
    // ---- ビット操作 ----
    OpcodeEntry::op("btst",  InsnHandler::Btst,    0x0000, ALL, sz::NONE, sz::NONE),
    OpcodeEntry::op("bset",  InsnHandler::BchClSt, 0x00C0, ALL, sz::NONE, sz::NONE),
    OpcodeEntry::op("bclr",  InsnHandler::BchClSt, 0x0080, ALL, sz::NONE, sz::NONE),
    OpcodeEntry::op("bchg",  InsnHandler::BchClSt, 0x0040, ALL, sz::NONE, sz::NONE),
    // ---- シフト / ローテート ----
    OpcodeEntry::op("asr",   InsnHandler::SftRot,  0xE000, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("asl",   InsnHandler::Asl,     0xE100, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("lsr",   InsnHandler::SftRot,  0xE008, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("lsl",   InsnHandler::SftRot,  0xE108, ALL, sz::BWL,  sz::L),
    OpcodeEntry::op("ror",   InsnHandler::SftRot,  0xE018, M68K, sz::BWL, sz::NONE),
    OpcodeEntry::op("rol",   InsnHandler::SftRot,  0xE118, M68K, sz::BWL, sz::NONE),
    OpcodeEntry::op("roxr",  InsnHandler::SftRot,  0xE010, M68K, sz::BWL, sz::NONE),
    OpcodeEntry::op("roxl",  InsnHandler::SftRot,  0xE110, M68K, sz::BWL, sz::NONE),
    // ---- 分岐 ----
    OpcodeEntry::op("bra",   InsnHandler::Bcc, 0x6000, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bt",    InsnHandler::Bcc, 0x6000, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bsr",   InsnHandler::Bcc, 0x6100, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bhi",   InsnHandler::Bcc, 0x6200, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bls",   InsnHandler::Bcc, 0x6300, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bcc",   InsnHandler::Bcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bhs",   InsnHandler::Bcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bcs",   InsnHandler::Bcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("blo",   InsnHandler::Bcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bne",   InsnHandler::Bcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnz",   InsnHandler::Bcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("beq",   InsnHandler::Bcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bze",   InsnHandler::Bcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bvc",   InsnHandler::Bcc, 0x6800, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bvs",   InsnHandler::Bcc, 0x6900, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bpl",   InsnHandler::Bcc, 0x6A00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bmi",   InsnHandler::Bcc, 0x6B00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bge",   InsnHandler::Bcc, 0x6C00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("blt",   InsnHandler::Bcc, 0x6D00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bgt",   InsnHandler::Bcc, 0x6E00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("ble",   InsnHandler::Bcc, 0x6F00, ALL, sz::BWLS, sz::BWLS),
    // bnls..bngt エイリアス（逆条件）
    OpcodeEntry::op("bnls",  InsnHandler::Bcc, 0x6200, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnhi",  InsnHandler::Bcc, 0x6300, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bncs",  InsnHandler::Bcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnlo",  InsnHandler::Bcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bncc",  InsnHandler::Bcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnhs",  InsnHandler::Bcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bneq",  InsnHandler::Bcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnze",  InsnHandler::Bcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnne",  InsnHandler::Bcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnnz",  InsnHandler::Bcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnvs",  InsnHandler::Bcc, 0x6800, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnvc",  InsnHandler::Bcc, 0x6900, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnmi",  InsnHandler::Bcc, 0x6A00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnpl",  InsnHandler::Bcc, 0x6B00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnlt",  InsnHandler::Bcc, 0x6C00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnge",  InsnHandler::Bcc, 0x6D00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bnle",  InsnHandler::Bcc, 0x6E00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("bngt",  InsnHandler::Bcc, 0x6F00, ALL, sz::BWLS, sz::BWLS),
    // JBRA 系（長距離分岐拡張）
    OpcodeEntry::op("jbra",  InsnHandler::JBcc, 0x6000, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbt",   InsnHandler::JBcc, 0x6000, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbsr",  InsnHandler::JBcc, 0x6100, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbhi",  InsnHandler::JBcc, 0x6200, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbls",  InsnHandler::JBcc, 0x6300, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbcc",  InsnHandler::JBcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbhs",  InsnHandler::JBcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbcs",  InsnHandler::JBcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jblo",  InsnHandler::JBcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbne",  InsnHandler::JBcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnz",  InsnHandler::JBcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbeq",  InsnHandler::JBcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbze",  InsnHandler::JBcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbvc",  InsnHandler::JBcc, 0x6800, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbvs",  InsnHandler::JBcc, 0x6900, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbpl",  InsnHandler::JBcc, 0x6A00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbmi",  InsnHandler::JBcc, 0x6B00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbge",  InsnHandler::JBcc, 0x6C00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jblt",  InsnHandler::JBcc, 0x6D00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbgt",  InsnHandler::JBcc, 0x6E00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jble",  InsnHandler::JBcc, 0x6F00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnls", InsnHandler::JBcc, 0x6200, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnhi", InsnHandler::JBcc, 0x6300, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbncs", InsnHandler::JBcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnlo", InsnHandler::JBcc, 0x6400, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbncc", InsnHandler::JBcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnhs", InsnHandler::JBcc, 0x6500, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbneq", InsnHandler::JBcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnze", InsnHandler::JBcc, 0x6700, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnne", InsnHandler::JBcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnnz", InsnHandler::JBcc, 0x6600, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnvs", InsnHandler::JBcc, 0x6800, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnvc", InsnHandler::JBcc, 0x6900, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnmi", InsnHandler::JBcc, 0x6A00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnpl", InsnHandler::JBcc, 0x6B00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnlt", InsnHandler::JBcc, 0x6C00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnge", InsnHandler::JBcc, 0x6D00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbnle", InsnHandler::JBcc, 0x6E00, ALL, sz::BWLS, sz::BWLS),
    OpcodeEntry::op("jbngt", InsnHandler::JBcc, 0x6F00, ALL, sz::BWLS, sz::BWLS),
    // DBcc
    OpcodeEntry::op("dbra",  InsnHandler::DBcc, 0x51C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbf",   InsnHandler::DBcc, 0x51C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbt",   InsnHandler::DBcc, 0x50C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbhi",  InsnHandler::DBcc, 0x52C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbls",  InsnHandler::DBcc, 0x53C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbcc",  InsnHandler::DBcc, 0x54C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbhs",  InsnHandler::DBcc, 0x54C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbcs",  InsnHandler::DBcc, 0x55C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dblo",  InsnHandler::DBcc, 0x55C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbne",  InsnHandler::DBcc, 0x56C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnz",  InsnHandler::DBcc, 0x56C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbeq",  InsnHandler::DBcc, 0x57C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbze",  InsnHandler::DBcc, 0x57C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbvc",  InsnHandler::DBcc, 0x58C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbvs",  InsnHandler::DBcc, 0x59C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbpl",  InsnHandler::DBcc, 0x5AC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbmi",  InsnHandler::DBcc, 0x5BC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbge",  InsnHandler::DBcc, 0x5CC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dblt",  InsnHandler::DBcc, 0x5DC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbgt",  InsnHandler::DBcc, 0x5EC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dble",  InsnHandler::DBcc, 0x5FC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnf",  InsnHandler::DBcc, 0x50C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnt",  InsnHandler::DBcc, 0x51C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnls", InsnHandler::DBcc, 0x52C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnhi", InsnHandler::DBcc, 0x53C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbncs", InsnHandler::DBcc, 0x54C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnlo", InsnHandler::DBcc, 0x54C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbncc", InsnHandler::DBcc, 0x55C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnhs", InsnHandler::DBcc, 0x55C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbneq", InsnHandler::DBcc, 0x57C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnze", InsnHandler::DBcc, 0x57C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnne", InsnHandler::DBcc, 0x56C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnnz", InsnHandler::DBcc, 0x56C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnvs", InsnHandler::DBcc, 0x58C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnvc", InsnHandler::DBcc, 0x59C8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnmi", InsnHandler::DBcc, 0x5AC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnpl", InsnHandler::DBcc, 0x5BC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnlt", InsnHandler::DBcc, 0x5CC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnge", InsnHandler::DBcc, 0x5DC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbnle", InsnHandler::DBcc, 0x5EC8, M68K, sz::W, sz::NONE),
    OpcodeEntry::op("dbngt", InsnHandler::DBcc, 0x5FC8, M68K, sz::W, sz::NONE),
    // Scc
    OpcodeEntry::op("st",    InsnHandler::Scc, 0x50C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sf",    InsnHandler::Scc, 0x51C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("shi",   InsnHandler::Scc, 0x52C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sls",   InsnHandler::Scc, 0x53C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("scc",   InsnHandler::Scc, 0x54C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("shs",   InsnHandler::Scc, 0x54C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("scs",   InsnHandler::Scc, 0x55C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("slo",   InsnHandler::Scc, 0x55C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sne",   InsnHandler::Scc, 0x56C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snz",   InsnHandler::Scc, 0x56C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("seq",   InsnHandler::Scc, 0x57C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sze",   InsnHandler::Scc, 0x57C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("svc",   InsnHandler::Scc, 0x58C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("svs",   InsnHandler::Scc, 0x59C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("spl",   InsnHandler::Scc, 0x5AC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("smi",   InsnHandler::Scc, 0x5BC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sge",   InsnHandler::Scc, 0x5CC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("slt",   InsnHandler::Scc, 0x5DC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sgt",   InsnHandler::Scc, 0x5EC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sle",   InsnHandler::Scc, 0x5FC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snf",   InsnHandler::Scc, 0x50C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snt",   InsnHandler::Scc, 0x51C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snls",  InsnHandler::Scc, 0x52C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snhi",  InsnHandler::Scc, 0x53C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sncc",  InsnHandler::Scc, 0x54C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snhs",  InsnHandler::Scc, 0x54C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sncs",  InsnHandler::Scc, 0x55C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snlo",  InsnHandler::Scc, 0x55C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sneq",  InsnHandler::Scc, 0x57C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snze",  InsnHandler::Scc, 0x57C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snne",  InsnHandler::Scc, 0x56C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snnz",  InsnHandler::Scc, 0x56C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snvs",  InsnHandler::Scc, 0x58C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snvc",  InsnHandler::Scc, 0x59C0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snmi",  InsnHandler::Scc, 0x5AC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snpl",  InsnHandler::Scc, 0x5BC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snge",  InsnHandler::Scc, 0x5CC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snlt",  InsnHandler::Scc, 0x5DC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("sngt",  InsnHandler::Scc, 0x5EC0, ALL, sz::B, sz::B),
    OpcodeEntry::op("snle",  InsnHandler::Scc, 0x5FC0, ALL, sz::B, sz::B),
    // フロー制御
    OpcodeEntry::op("link",  InsnHandler::Link,    0x4E50, ALL, sz::WL, sz::W),
    OpcodeEntry::noop("unlk",InsnHandler::Unlk,    0x4E58, ALL),
    OpcodeEntry::op("trap",  InsnHandler::Trap,    0x4E40, ALL, sz::NONE, sz::NONE),
    OpcodeEntry::noop("stop",InsnHandler::StopRtd, 0x4E72, ALL),
    OpcodeEntry::op("tas",   InsnHandler::Scc,     0x4AC0, CpuMask(
        crate::options::cpu::C000|crate::options::cpu::C010|crate::options::cpu::C020|
        crate::options::cpu::C030|crate::options::cpu::C040|crate::options::cpu::C060|
        crate::options::cpu::C540
    ), sz::B, sz::B),
    // ---- 68010+ 拡張命令 ----
    OpcodeEntry::op("rtd",   InsnHandler::StopRtd, 0x4E74, C010_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bkpt",  InsnHandler::Bkpt,    0x4848, C010_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("extb",  InsnHandler::ExtB,    0x49C0, CpuMask(
        crate::options::cpu::C020|crate::options::cpu::C030|crate::options::cpu::C040|
        crate::options::cpu::C060|crate::options::cpu::C520|crate::options::cpu::C530|
        crate::options::cpu::C540
    ), sz::L, sz::L),
    OpcodeEntry::op("moves", InsnHandler::MovesInsn, 0x0E00, C010_UP, sz::BWL, sz::NONE),
    OpcodeEntry::op("movec", InsnHandler::MoveC,   0x4E7A, C010_UP, sz::L, sz::L),
    // ---- 68020+ 拡張命令 ----
    OpcodeEntry::op("bftst",  InsnHandler::BfChgClrSet, 0xE8C0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfextu", InsnHandler::BfExtFfo,    0xE9C0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfchg",  InsnHandler::BfChgClrSet, 0xEAC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfexts", InsnHandler::BfExtFfo,    0xEBC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfclr",  InsnHandler::BfChgClrSet, 0xECC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfffo",  InsnHandler::BfExtFfo,    0xEDC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfset",  InsnHandler::BfChgClrSet, 0xEEC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("bfins",  InsnHandler::BfIns,       0xEFC0, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("pack",   InsnHandler::PackUnpk,    0x8140, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("unpk",   InsnHandler::PackUnpk,    0x8180, C020_UP, sz::NONE, sz::NONE),
    OpcodeEntry::op("cas",    InsnHandler::CasInsn,     0x08C0, C020_UP, sz::BWL, sz::NONE),
    OpcodeEntry::op("cas2",   InsnHandler::Cas2Insn,    0x0CFC, C020_UP, sz::WL,  sz::NONE),
    OpcodeEntry::op("divsl",  InsnHandler::DivSlUl,     0x4C41, C020_UP, sz::L,   sz::NONE),
    OpcodeEntry::op("divul",  InsnHandler::DivSlUl,     0x4C40, C020_UP, sz::L,   sz::NONE),
    OpcodeEntry::op("cmp2",   InsnHandler::CmpChk2,     0x0000, C020_UP, sz::BWL, sz::NONE),
    OpcodeEntry::op("chk2",   InsnHandler::CmpChk2,     0x0800, C020_UP, sz::BWL, sz::NONE),
    // TRAPcc バリアント
    OpcodeEntry::op("trapt",   InsnHandler::Trapcc, 0x50F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapf",   InsnHandler::Trapcc, 0x51F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("traphi",  InsnHandler::Trapcc, 0x52F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapls",  InsnHandler::Trapcc, 0x53F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapcc",  InsnHandler::Trapcc, 0x54F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("traphs",  InsnHandler::Trapcc, 0x54F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapcs",  InsnHandler::Trapcc, 0x55F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("traplo",  InsnHandler::Trapcc, 0x55F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapne",  InsnHandler::Trapcc, 0x56F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapnz",  InsnHandler::Trapcc, 0x56F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapeq",  InsnHandler::Trapcc, 0x57F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapze",  InsnHandler::Trapcc, 0x57F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapvc",  InsnHandler::Trapcc, 0x58F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapvs",  InsnHandler::Trapcc, 0x59F8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trappl",  InsnHandler::Trapcc, 0x5AF8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapmi",  InsnHandler::Trapcc, 0x5BF8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapge",  InsnHandler::Trapcc, 0x5CF8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("traplt",  InsnHandler::Trapcc, 0x5DF8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("trapgt",  InsnHandler::Trapcc, 0x5EF8, C020_UP, sz::WL, sz::NONE),
    OpcodeEntry::op("traple",  InsnHandler::Trapcc, 0x5FF8, C020_UP, sz::WL, sz::NONE),
    // ---- 68040+ ----
    OpcodeEntry::op("move16",  InsnHandler::Move16Insn, 0xF600, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cinvl",   InsnHandler::CInvPushLP, 0xF408, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cinvp",   InsnHandler::CInvPushLP, 0xF410, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cinva",   InsnHandler::CInvPushA,  0xF418, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cpushl",  InsnHandler::CInvPushLP, 0xF428, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::C520|
        crate::options::cpu::C530|crate::options::cpu::C540
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cpushp",  InsnHandler::CInvPushLP, 0xF430, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("cpusha",  InsnHandler::CInvPushA,  0xF438, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::C520|
        crate::options::cpu::C530|crate::options::cpu::C540
    ), sz::NONE, sz::NONE),
    // ---- FPU (68881/68882) ----
    OpcodeEntry::op("fmove",    InsnHandler::FMove,    0xF200, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fmovem",   InsnHandler::FMoveM,   0xF200, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fadd",     InsnHandler::FArith,   0x0022, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fsub",     InsnHandler::FArith,   0x0028, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fmul",     InsnHandler::FArith,   0x0023, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fdiv",     InsnHandler::FArith,   0x0020, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fcmp",     InsnHandler::FCmp,     0x0038, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("ftst",     InsnHandler::FTst,     0x003A, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fmovecr",  InsnHandler::FMoveCr,  0x5C00, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::X, sz::NONE),
    OpcodeEntry::op("fsincos",  InsnHandler::FSinCos,  0x0030, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0), sz::NONE),
    OpcodeEntry::op("fsave",    InsnHandler::FSave,    0xF300, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("frestore", InsnHandler::FRestore, 0xF340, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::noop("fnop",   InsnHandler::FNop,     0xF080, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    )),
    // FBcc
    OpcodeEntry::op("fbf",      InsnHandler::FBcc,     0xF080, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbeq",     InsnHandler::FBcc,     0xF081, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbogt",    InsnHandler::FBcc,     0xF082, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fboge",    InsnHandler::FBcc,     0xF083, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbolt",    InsnHandler::FBcc,     0xF084, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbole",    InsnHandler::FBcc,     0xF085, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbogl",    InsnHandler::FBcc,     0xF086, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbor",     InsnHandler::FBcc,     0xF087, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbun",     InsnHandler::FBcc,     0xF088, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbueq",    InsnHandler::FBcc,     0xF089, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbugt",    InsnHandler::FBcc,     0xF08A, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbuge",    InsnHandler::FBcc,     0xF08B, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbult",    InsnHandler::FBcc,     0xF08C, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbule",    InsnHandler::FBcc,     0xF08D, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbne",     InsnHandler::FBcc,     0xF08E, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbt",      InsnHandler::FBcc,     0xF08F, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbra",     InsnHandler::FBcc,     0xF08F, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbsf",     InsnHandler::FBcc,     0xF090, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbseq",    InsnHandler::FBcc,     0xF091, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbgt",     InsnHandler::FBcc,     0xF092, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbge",     InsnHandler::FBcc,     0xF093, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fblt",     InsnHandler::FBcc,     0xF094, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fble",     InsnHandler::FBcc,     0xF095, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbgl",     InsnHandler::FBcc,     0xF096, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbgle",    InsnHandler::FBcc,     0xF097, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbngle",   InsnHandler::FBcc,     0xF098, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbngl",    InsnHandler::FBcc,     0xF099, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbnle",    InsnHandler::FBcc,     0xF09A, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbnlt",    InsnHandler::FBcc,     0xF09B, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbnge",    InsnHandler::FBcc,     0xF09C, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbngt",    InsnHandler::FBcc,     0xF09D, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbsne",    InsnHandler::FBcc,     0xF09E, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    OpcodeEntry::op("fbst",     InsnHandler::FBcc,     0xF09F, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::WL, sz::NONE),
    // FDBcc
    OpcodeEntry::op("fdbf",     InsnHandler::FDBcc,    0x0000, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbra",    InsnHandler::FDBcc,    0x0000, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbeq",    InsnHandler::FDBcc,    0x0001, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbogt",   InsnHandler::FDBcc,    0x0002, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdboge",   InsnHandler::FDBcc,    0x0003, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbolt",   InsnHandler::FDBcc,    0x0004, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbole",   InsnHandler::FDBcc,    0x0005, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbogl",   InsnHandler::FDBcc,    0x0006, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbor",    InsnHandler::FDBcc,    0x0007, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbun",    InsnHandler::FDBcc,    0x0008, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbueq",   InsnHandler::FDBcc,    0x0009, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbugt",   InsnHandler::FDBcc,    0x000A, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbuge",   InsnHandler::FDBcc,    0x000B, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbult",   InsnHandler::FDBcc,    0x000C, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbule",   InsnHandler::FDBcc,    0x000D, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbne",    InsnHandler::FDBcc,    0x000E, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbt",     InsnHandler::FDBcc,    0x000F, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbsf",    InsnHandler::FDBcc,    0x0010, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbseq",   InsnHandler::FDBcc,    0x0011, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbgt",    InsnHandler::FDBcc,    0x0012, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbge",    InsnHandler::FDBcc,    0x0013, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdblt",    InsnHandler::FDBcc,    0x0014, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdble",    InsnHandler::FDBcc,    0x0015, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbgl",    InsnHandler::FDBcc,    0x0016, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbgle",   InsnHandler::FDBcc,    0x0017, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbngle",  InsnHandler::FDBcc,    0x0018, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbngl",   InsnHandler::FDBcc,    0x0019, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbnle",   InsnHandler::FDBcc,    0x001A, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbnlt",   InsnHandler::FDBcc,    0x001B, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbnge",   InsnHandler::FDBcc,    0x001C, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbngt",   InsnHandler::FDBcc,    0x001D, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbsne",   InsnHandler::FDBcc,    0x001E, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    OpcodeEntry::op("fdbst",    InsnHandler::FDBcc,    0x001F, CpuMask(
        crate::options::cpu::C040|crate::options::cpu::C060|crate::options::cpu::CFPP
    ), sz::NONE, sz::NONE),
    // 無操作命令（no-operand）はoptbln相当
    OpcodeEntry::noop("nop",     InsnHandler::Bcc,  0x4E71, ALL),
    OpcodeEntry::noop("rts",     InsnHandler::Bcc,  0x4E75, ALL),
    OpcodeEntry::noop("rtr",     InsnHandler::Bcc,  0x4E77, M68K),
    OpcodeEntry::noop("rte",     InsnHandler::Bcc,  0x4E73, M68K),
    OpcodeEntry::noop("trapv",   InsnHandler::Bcc,  0x4E76, M68K),
    OpcodeEntry::noop("illegal", InsnHandler::Bcc,  0x4AFC, ALL),
    OpcodeEntry::noop("reset",   InsnHandler::Bcc,  0x4E70, M68K),
    // ---- 疑似命令 ----
    OpcodeEntry::pseudo("even",     InsnHandler::Even),
    OpcodeEntry::pseudo("quad",     InsnHandler::Quad),
    OpcodeEntry::pseudo("align",    InsnHandler::Align),
    OpcodeEntry::pseudos("dc",  InsnHandler::Dc,
        SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0)),
    OpcodeEntry::pseudos("ds",  InsnHandler::Ds,
        SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0)),
    OpcodeEntry::pseudos("dcb", InsnHandler::Dcb,
        SizeFlags(sz::B.0|sz::W.0|sz::L.0|sz::S.0|sz::D.0|sz::X.0|sz::P.0)),
    OpcodeEntry::pseudo("equ",     InsnHandler::Equ),
    OpcodeEntry::pseudo("set",     InsnHandler::Set),
    OpcodeEntry::pseudo("reg",     InsnHandler::Reg),
    OpcodeEntry::pseudo("rept",    InsnHandler::Rept),
    OpcodeEntry::pseudo("irp",     InsnHandler::Irp),
    OpcodeEntry::pseudo("irpc",    InsnHandler::Irpc),
    OpcodeEntry::pseudo("xdef",    InsnHandler::Xdef),
    OpcodeEntry::pseudo("xref",    InsnHandler::Xref),
    OpcodeEntry::pseudo("globl",   InsnHandler::Globl),
    OpcodeEntry::pseudo("entry",   InsnHandler::Xdef),
    OpcodeEntry::pseudo("public",  InsnHandler::Xdef),
    OpcodeEntry::pseudo("extrn",   InsnHandler::Xref),
    OpcodeEntry::pseudo("external",InsnHandler::Xref),
    OpcodeEntry::pseudo("global",  InsnHandler::Globl),
    OpcodeEntry::pseudo("text",    InsnHandler::TextSect),
    OpcodeEntry::pseudo("data",    InsnHandler::DataSect),
    OpcodeEntry::pseudo("bss",     InsnHandler::BssSect),
    OpcodeEntry::pseudo("comm",    InsnHandler::Comm),
    OpcodeEntry::pseudo("stack",   InsnHandler::Stack),
    OpcodeEntry::pseudo("offset",  InsnHandler::Offset),
    OpcodeEntry::pseudo("offsym",  InsnHandler::OffsymPs),
    OpcodeEntry::pseudo("macro",   InsnHandler::MacroDef),
    OpcodeEntry::pseudo("exitm",   InsnHandler::ExitM),
    OpcodeEntry::pseudo("endm",    InsnHandler::EndM),
    OpcodeEntry::pseudo("local",   InsnHandler::Local),
    OpcodeEntry::pseudo("sizem",   InsnHandler::SizeM),
    OpcodeEntry::pseudo("if",      InsnHandler::If),
    OpcodeEntry::pseudo("ifne",    InsnHandler::If),
    OpcodeEntry::pseudo("iff",     InsnHandler::Iff),
    OpcodeEntry::pseudo("ifeq",    InsnHandler::Iff),
    OpcodeEntry::pseudo("ifdef",   InsnHandler::Ifdef),
    OpcodeEntry::pseudo("ifndef",  InsnHandler::Ifndef),
    OpcodeEntry::pseudo("else",    InsnHandler::Else),
    OpcodeEntry::pseudo("elseif",  InsnHandler::Elseif),
    OpcodeEntry::pseudo("elif",    InsnHandler::Elseif),
    OpcodeEntry::pseudo("endif",   InsnHandler::Endif),
    OpcodeEntry::pseudo("endc",    InsnHandler::Endif),
    OpcodeEntry::pseudo("end",     InsnHandler::End),
    OpcodeEntry::pseudo("insert",  InsnHandler::Insert),
    OpcodeEntry::pseudo("include", InsnHandler::Include),
    OpcodeEntry::pseudo("request", InsnHandler::Request),
    OpcodeEntry::pseudo("list",    InsnHandler::List),
    OpcodeEntry::pseudo("nlist",   InsnHandler::Nlist),
    OpcodeEntry::pseudo("lall",    InsnHandler::Lall),
    OpcodeEntry::pseudo("sall",    InsnHandler::Sall),
    OpcodeEntry::pseudo("width",   InsnHandler::Width),
    OpcodeEntry::pseudo("page",    InsnHandler::Page),
    OpcodeEntry::pseudo("title",   InsnHandler::Title),
    OpcodeEntry::pseudo("subttl",  InsnHandler::SubTtl),
    OpcodeEntry::pseudo("fail",    InsnHandler::Fail),
    OpcodeEntry::pseudo("cpu",     InsnHandler::Cpu),
    // 相対セクション
    OpcodeEntry::pseudo("rdata",   InsnHandler::RdataSect),
    OpcodeEntry::pseudo("rbss",    InsnHandler::RbssSect),
    OpcodeEntry::pseudo("rstack",  InsnHandler::RstackSect),
    OpcodeEntry::pseudo("rcomm",   InsnHandler::Rcomm),
    OpcodeEntry::pseudo("rldata",  InsnHandler::RldataSect),
    OpcodeEntry::pseudo("rlbss",   InsnHandler::RlbssSect),
    OpcodeEntry::pseudo("rlstack", InsnHandler::RlstackSect),
    OpcodeEntry::pseudo("rlcomm",  InsnHandler::Rlcomm),
    // CPU 指定
    OpcodeEntry::pseudo("68000",   InsnHandler::Cpu68000),
    OpcodeEntry::pseudo("68010",   InsnHandler::Cpu68010),
    OpcodeEntry::pseudo("68020",   InsnHandler::Cpu68020),
    OpcodeEntry::pseudo("68030",   InsnHandler::Cpu68030),
    OpcodeEntry::pseudo("68040",   InsnHandler::Cpu68040),
    OpcodeEntry::pseudo("68060",   InsnHandler::Cpu68060),
    OpcodeEntry::pseudo("5200",    InsnHandler::Cpu5200),
    OpcodeEntry::pseudo("5300",    InsnHandler::Cpu5300),
    OpcodeEntry::pseudo("5400",    InsnHandler::Cpu5400),
    OpcodeEntry::pseudo("fpid",    InsnHandler::FpId),
    OpcodeEntry::pseudo("pragma",  InsnHandler::Pragma),
    // SCD デバッグ情報（Phase 10）
    OpcodeEntry::pseudo("file",    InsnHandler::FileScd),
    OpcodeEntry::pseudo("def",     InsnHandler::Def),
    OpcodeEntry::pseudo("endef",   InsnHandler::Endef),
    OpcodeEntry::pseudo("val",     InsnHandler::Val),
    OpcodeEntry::pseudo("scl",     InsnHandler::Scl),
    OpcodeEntry::pseudo("type",    InsnHandler::TypeScd),
    OpcodeEntry::pseudo("tag",     InsnHandler::Tag),
    OpcodeEntry::pseudo("ln",      InsnHandler::Ln),
    OpcodeEntry::pseudo("line",    InsnHandler::Line),
    OpcodeEntry::pseudo("size",    InsnHandler::SizeScd),
    OpcodeEntry::pseudo("dim",     InsnHandler::Dim),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::cpu;

    fn make_tbl() -> SymbolTable {
        SymbolTable::new(false)
    }

    #[test]
    fn test_lookup_register_d0() {
        let tbl = make_tbl();
        let sym = tbl.lookup_reg(b"d0", cpu::C000);
        assert!(sym.is_some());
        if let Some(Symbol::Register { arch, regno }) = sym {
            assert!(arch.matches(cpu::C000));
            assert_eq!(*regno, reg::D0);
        } else {
            panic!("expected Register");
        }
    }

    #[test]
    fn test_lookup_register_case_insensitive() {
        let tbl = make_tbl();
        assert!(tbl.lookup_reg(b"D0", cpu::C000).is_some());
        assert!(tbl.lookup_reg(b"A7", cpu::C000).is_some());
        assert!(tbl.lookup_reg(b"SP", cpu::C000).is_some());
    }

    #[test]
    fn test_lookup_register_cpu_filter() {
        let tbl = make_tbl();
        // fp0 は 68040 以降のみ
        assert!(tbl.lookup_reg(b"fp0", cpu::C000).is_none());
        assert!(tbl.lookup_reg(b"fp0", cpu::C040).is_some());
    }

    #[test]
    fn test_lookup_opcode_move() {
        let tbl = make_tbl();
        let sym = tbl.lookup_cmd(b"move", cpu::C000);
        assert!(sym.is_some());
        if let Some(Symbol::Opcode { handler, opcode, .. }) = sym {
            assert_eq!(*handler, InsnHandler::Move);
            assert_eq!(*opcode, 0x0000);
        } else {
            panic!("expected Opcode");
        }
    }

    #[test]
    fn test_lookup_opcode_case_insensitive() {
        let tbl = make_tbl();
        assert!(tbl.lookup_cmd(b"MOVE", cpu::C000).is_some());
        assert!(tbl.lookup_cmd(b"BRA",  cpu::C000).is_some());
        assert!(tbl.lookup_cmd(b"NOP",  cpu::C000).is_some());
    }

    #[test]
    fn test_lookup_pseudo() {
        let tbl = make_tbl();
        // 疑似命令は全CPUで使用可能
        let sym = tbl.lookup_cmd(b"dc", cpu::C000);
        assert!(sym.is_some());
        assert!(sym.unwrap().is_pseudo());
        // 68060 でも使用可能
        assert!(tbl.lookup_cmd(b"dc", cpu::C060).is_some());
    }

    #[test]
    fn test_lookup_dbcc_cpu_filter() {
        let tbl = make_tbl();
        // dbcc は 68k のみ、ColdFire 不可
        assert!(tbl.lookup_cmd(b"dbra", cpu::C000).is_some());
        assert!(tbl.lookup_cmd(b"dbra", cpu::C060).is_some());
        assert!(tbl.lookup_cmd(b"dbra", cpu::C520).is_none());
    }

    #[test]
    fn test_define_and_lookup_user_sym() {
        let mut tbl = make_tbl();
        tbl.define(
            b"MY_LABEL".to_vec(),
            Symbol::Value {
                attrib: DefAttrib::Define,
                ext_attrib: ExtAttrib::None,
                section: 1,
                org_num: 0,
                first: FirstDef::Other,
                opt_count: 0,
                value: 0x1000,
            },
        );
        assert!(tbl.is_defined(b"MY_LABEL"));
        assert!(!tbl.is_defined(b"my_label")); // 大文字小文字区別
    }

    #[test]
    fn test_builtin_counts() {
        let tbl = make_tbl();
        // レジスタ・命令のカウントが非ゼロであることを確認
        assert!(tbl.reg_count() > 50);   // 最低 50 レジスタ
        assert!(tbl.cmd_count() > 100);  // 最低 100 命令
    }

    #[test]
    fn test_sym_len8() {
        let tbl = SymbolTable::new(true);  // -8 オプション有効
        // 8文字で切り詰め
        tbl.lookup_sym(b"ABCDEFGHIJKLMN"); // クラッシュしないこと
    }
}
