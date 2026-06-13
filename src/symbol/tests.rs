use super::types::{reg, sz, CpuMask, DefAttrib, ExtAttrib, FirstDef, InsnHandler, Symbol};
use super::SymbolTable;
use crate::options::cpu;

// =================================================================
// mod.rs テスト群 (シンボルテーブルの基本操作)
// =================================================================

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
    if let Some(Symbol::Opcode {
        handler, opcode, ..
    }) = sym
    {
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
    assert!(tbl.lookup_cmd(b"BRA", cpu::C000).is_some());
    assert!(tbl.lookup_cmd(b"NOP", cpu::C000).is_some());
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
    assert!(tbl.reg_count() > 50); // 最低 50 レジスタ
    assert!(tbl.cmd_count() > 100); // 最低 100 命令
}

#[test]
fn test_sym_len8() {
    let tbl = SymbolTable::new(true); // -8 オプション有効
                                      // 8文字で切り詰め
    tbl.lookup_sym(b"ABCDEFGHIJKLMN"); // クラッシュしないこと
}

// =================================================================
// types.rs テスト群 (サイズ・CPUマスク等の動作確認)
// =================================================================

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
