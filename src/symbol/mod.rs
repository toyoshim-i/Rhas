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
mod table;

use std::collections::HashMap;
use crate::utils;
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
        for (name, arch, regno) in table::REGISTER_TABLE {
            let key = utils::to_lowercase_vec(name.as_bytes());
            self.reg_table.insert(
                key,
                Symbol::Register { arch: CpuMask(*arch), regno: *regno },
            );
        }
        // MOVEC control register names (68010+)
        for (name, val) in [
            ("sfc",  0x000i32), ("dfc",  0x001),
            ("cacr", 0x002), ("tc",   0x003),
            ("itt0", 0x004), ("itt1", 0x005),
            ("dtt0", 0x006), ("dtt1", 0x007),
            ("buscr", 0x008),
            ("usp",  0x800), ("vbr",  0x801),
            ("caar", 0x802), ("msp",  0x803),
            ("isp",  0x804), ("mmusr", 0x805),
            ("urp",  0x806), ("srp",  0x807),
            ("pcr",  0x808),
        ] {
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
        for entry in table::OPCODE_TABLE {
            let key = utils::to_lowercase_vec(entry.name.as_bytes());
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
        let key = utils::to_lowercase_vec(name);
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
        let key = utils::to_lowercase_vec(self.truncate_if_len8(name));
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
        let key = utils::to_lowercase_vec(name);
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
