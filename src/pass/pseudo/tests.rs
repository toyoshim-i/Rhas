use crate::context::{AssemblyContext, Section};
use crate::pass::temp::TempRecord;
use crate::symbol::types::InsnHandler;

// =================================================================
// section.rs テスト群
// =================================================================

use super::section::handle_section;

#[test]
fn test_handle_section_text() {
    let mut ctx = AssemblyContext::new(crate::options::Options::default());
    let mut records = Vec::new();

    handle_section(InsnHandler::TextSect, &mut ctx, &mut records);

    assert_eq!(ctx.section, Section::Text);
    assert_eq!(records.len(), 1);
    if let TempRecord::SectChange { id } = &records[0] {
        assert_eq!(*id, 0x01u8);
    } else {
        panic!("Expected SectChange record");
    }
}

#[test]
fn test_handle_section_data() {
    let mut ctx = AssemblyContext::new(crate::options::Options::default());
    let mut records = Vec::new();

    handle_section(InsnHandler::DataSect, &mut ctx, &mut records);

    assert_eq!(ctx.section, Section::Data);
}

// =================================================================
// macro_.rs テスト群
// =================================================================

use super::macro_::{check_macro_nesting, parse_repeat_count};

#[test]
fn test_parse_repeat_count_zero() {
    assert_eq!(parse_repeat_count(0), 0);
}

#[test]
fn test_parse_repeat_count_nonzero() {
    assert_eq!(parse_repeat_count(10), 10);
    assert_eq!(parse_repeat_count(1), 1);
}

#[test]
fn test_check_macro_nesting_within() {
    assert!(check_macro_nesting(2, 10));
    assert!(check_macro_nesting(9, 10));
}

#[test]
fn test_check_macro_nesting_full() {
    assert!(!check_macro_nesting(10, 10));
    assert!(!check_macro_nesting(11, 10));
}

#[test]
fn test_check_macro_nesting_zero() {
    assert!(check_macro_nesting(0, 1));
}

// =================================================================
// conditional.rs テスト群
// =================================================================

use super::conditional::{
    handle_conditional, handle_skip, read_ident, set_if_matched, skip_spaces,
};

#[test]
fn test_read_ident_simple() {
    let line = b"LABEL rest";
    let mut pos = 0;
    let ident = read_ident(line, &mut pos);
    assert_eq!(ident, b"LABEL");
    assert_eq!(pos, 5);
}

#[test]
fn test_read_ident_with_spaces() {
    let line = b"  SYMBOL  more";
    let mut pos = 0;
    let ident = read_ident(line, &mut pos);
    assert_eq!(ident, b"SYMBOL");
}

#[test]
fn test_skip_spaces() {
    let line = b"   text";
    let mut pos = 0;
    skip_spaces(line, &mut pos);
    assert_eq!(pos, 3);
}

#[test]
fn test_set_if_matched_in_bounds() {
    let mut arr = [false, false, false];
    set_if_matched(&mut arr, 1, true);
    assert_eq!(arr[1], true);
    assert_eq!(arr[0], false);
}

#[test]
fn test_set_if_matched_out_of_bounds() {
    let mut arr = [false, false];
    set_if_matched(&mut arr, 10, true); // Should not crash
    assert_eq!(arr[0], false);
}

#[test]
fn test_handle_conditional_if_true() {
    let mut sym = crate::symbol::SymbolTable::new(false);
    let mut ctx = crate::context::AssemblyContext::new(crate::options::Options::default());
    let mut p1 = crate::pass::pass1::P1Ctx::new(&mut sym, &mut ctx);
    let mut pos = 0;
    let line = b"1";
    handle_conditional(InsnHandler::If, line, &mut pos, &mut p1);
    assert_eq!(p1.if_nest, 1);
    assert!(p1.if_matched[1]);
    assert!(!p1.is_skip);
}

#[test]
fn test_handle_conditional_if_false() {
    let mut sym = crate::symbol::SymbolTable::new(false);
    let mut ctx = crate::context::AssemblyContext::new(crate::options::Options::default());
    let mut p1 = crate::pass::pass1::P1Ctx::new(&mut sym, &mut ctx);
    let mut pos = 0;
    let line = b"0";
    handle_conditional(InsnHandler::If, line, &mut pos, &mut p1);
    assert_eq!(p1.if_nest, 1);
    assert!(!p1.if_matched[1]);
    assert!(p1.is_skip);
    assert_eq!(p1.skip_nest, 1);
}

#[test]
fn test_handle_skip_else_resumes() {
    let mut sym = crate::symbol::SymbolTable::new(false);
    let mut ctx = crate::context::AssemblyContext::new(crate::options::Options::default());
    let mut p1 = crate::pass::pass1::P1Ctx::new(&mut sym, &mut ctx);
    p1.is_skip = true;
    p1.if_nest = 1;
    p1.skip_nest = 1;
    p1.if_matched[1] = false;
    let mut pos = 0;
    let line = b"";
    handle_skip(Some(InsnHandler::Else), line, &mut pos, &mut p1);
    assert!(!p1.is_skip);
    assert!(p1.if_matched[1]);
}

// =================================================================
// debug.rs テスト群
// =================================================================

use super::debug::{is_valid_scd_record_type, parse_scd_filename, ScdRecordType};

#[test]
fn test_scd_record_type_variants() {
    assert_eq!(ScdRecordType::SourceLine as u8, 0x01);
    assert_eq!(ScdRecordType::SymbolDef as u8, 0x02);
    assert_eq!(ScdRecordType::SymbolRef as u8, 0x03);
}

#[test]
fn test_parse_scd_filename_simple() {
    let line = b"prog.s";
    let mut pos = 0;
    let fname = parse_scd_filename(line, &mut pos);
    assert_eq!(fname, b"prog.s");
    assert_eq!(pos, 6);
}

#[test]
fn test_parse_scd_filename_with_spaces() {
    let line = b"  debug.scd  more";
    let mut pos = 0;
    let fname = parse_scd_filename(line, &mut pos);
    assert_eq!(fname, b"debug.scd");
}

#[test]
fn test_is_valid_scd_record_type() {
    assert!(is_valid_scd_record_type(0x01));
    assert!(is_valid_scd_record_type(0x02));
    assert!(is_valid_scd_record_type(0x03));
    assert!(!is_valid_scd_record_type(0x00));
    assert!(!is_valid_scd_record_type(0x04));
    assert!(!is_valid_scd_record_type(0xFF));
}

// =================================================================
// misc.rs テスト群
// =================================================================

use super::misc::{
    handle_misc, is_visibility_directive, parse_org_address, AlignmentOperand, CpuDirective,
};

#[test]
fn test_cpu_directive_from_number() {
    assert_eq!(
        CpuDirective::from_number(68000),
        Some(CpuDirective::Cpu68000)
    );
    assert_eq!(
        CpuDirective::from_number(68020),
        Some(CpuDirective::Cpu68020)
    );
    assert_eq!(
        CpuDirective::from_number(68060),
        Some(CpuDirective::Cpu68060)
    );
    assert_eq!(CpuDirective::from_number(99999), None);
}

#[test]
fn test_cpu_directive_number() {
    assert_eq!(CpuDirective::Cpu68000.number(), 68000);
    assert_eq!(CpuDirective::Cpu68060.number(), 68060);
}

#[test]
fn test_cpu_supports_fpu() {
    assert!(!CpuDirective::Cpu68020.supports_fpu());
    assert!(CpuDirective::Cpu68040.supports_fpu());
    assert!(CpuDirective::Cpu68060.supports_fpu());
}

#[test]
fn test_alignment_operand_boundary() {
    assert_eq!(AlignmentOperand::Even.boundary(), 2);
    assert_eq!(AlignmentOperand::Quad.boundary(), 4);
    assert_eq!(AlignmentOperand::Octa.boundary(), 8);
    assert_eq!(AlignmentOperand::Hex.boundary(), 16);
}

#[test]
fn test_parse_org_address() {
    assert_eq!(parse_org_address(0x1000), 0x1000);
    assert_eq!(parse_org_address(0), 0);
}

#[test]
fn test_is_visibility_directive() {
    assert!(is_visibility_directive(b"globl"));
    assert!(is_visibility_directive(b"extern"));
    assert!(!is_visibility_directive(b"label"));
    assert!(!is_visibility_directive(b"text"));
}

#[test]
fn test_handle_misc_cpu() {
    let mut sym = crate::symbol::SymbolTable::new(false);
    let mut ctx = crate::context::AssemblyContext::new(crate::options::Options::default());
    let mut p1 = crate::pass::pass1::P1Ctx::new(&mut sym, &mut ctx);
    let mut records = Vec::new();
    let mut pos = 0;
    let line = b"68020";
    handle_misc(
        InsnHandler::Cpu,
        &None,
        line,
        &mut pos,
        &mut p1,
        &mut records,
    );
    assert_eq!(p1.ctx.cpu.number, 68020);
    assert_eq!(records.len(), 1);
    if let TempRecord::Cpu { cpu } = &records[0] {
        assert_eq!(cpu.number, 68020);
    } else {
        panic!("expected Cpu record");
    }
}

#[test]
fn test_handle_misc_globl_and_xref() {
    let mut sym = crate::symbol::SymbolTable::new(false);
    let mut ctx = crate::context::AssemblyContext::new(crate::options::Options::default());
    let mut p1 = crate::pass::pass1::P1Ctx::new(&mut sym, &mut ctx);
    let mut records = Vec::new();
    let mut pos = 0;
    let line = b"foo,bar";
    handle_misc(
        InsnHandler::Globl,
        &None,
        line,
        &mut pos,
        &mut p1,
        &mut records,
    );
    assert_eq!(records.len(), 2);
    assert!(matches!(records[0], TempRecord::Globl { .. }));
    assert!(matches!(records[1], TempRecord::Globl { .. }));

    // xref should insert undef symbols
    pos = 0;
    records.clear();
    let line2 = b"baz";
    handle_misc(
        InsnHandler::Xref,
        &None,
        line2,
        &mut pos,
        &mut p1,
        &mut records,
    );
    assert_eq!(records.len(), 1);
    assert!(p1.sym.lookup_sym(b"baz").is_some());
}
