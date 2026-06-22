mod common;
use common::*;

#[test]
fn test_c4_cmpi0_to_tst() {
    let src = b"\tcmpi.l\t#0,d3\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // TST.L D3 = 0x4A83
    assert_eq!(text.bytes, [0x4A, 0x83]);
}

#[test]
fn test_c4_movea_l_imm_to_w() {
    let src = b"\tmovea.l\t#1234,a2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // MOVEA.W #1234,A2 = 0x347C 0x04D2
    assert_eq!(text.bytes, [0x34, 0x7C, 0x04, 0xD2]);
}

#[test]
fn test_c4_asl_imm1_to_add() {
    let src = b"\tasl.w\t#1,d2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // ADD.W D2,D2 = 0xD442
    assert_eq!(text.bytes, [0xD4, 0x42]);
}

#[test]
fn test_c4_clr_l_does_not_optimize_on_68020_plus() {
    let src = b"\t.68040\n\tclr.l\td0\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // 68020+ では CLR.L Dn は MOVEQ #0,Dn に変換しない
    assert_eq!(text.bytes, [0x42, 0x80]);
}

#[test]
fn test_c4_cmpa_zero_to_tst_on_68020_plus() {
    let src = b"\t.68040\n\tcmpa.l\t#0,a2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // CMPA.L #0,A2 -> TST.L A2
    assert_eq!(text.bytes, [0x4A, 0x8A]);
}

#[test]
fn test_c4_lea_disp_to_addq() {
    let src = b"\tlea\t(4,a4),a4\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // LEA (4,A4),A4 -> ADDQ.W #4,A4
    assert_eq!(text.bytes, [0x58, 0x4C]);
}

// ---- ColdFire CPU 選択 ----

#[test]
fn test_coldfire_cpu5200_directive() {
    let src = b"\t.5200\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu.number, 5200);
    assert_ne!(ctx.cpu.features & rhas::options::cpu::C520, 0);
}

#[test]
fn test_coldfire_cpu5300_directive() {
    let src = b"\t.5300\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu.number, 5300);
    assert_ne!(ctx.cpu.features & rhas::options::cpu::C530, 0);
}

#[test]
fn test_coldfire_cpu5400_directive() {
    let src = b"\t.5400\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu.number, 5400);
    assert_ne!(ctx.cpu.features & rhas::options::cpu::C540, 0);
}

// ---- .cpu 式指定 ----

#[test]
fn test_cpu_directive_68020() {
    let src = b"\t.cpu\t68020\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu.number, 68020);
    assert_ne!(ctx.cpu.features & rhas::options::cpu::C020, 0);
}

#[test]
fn test_cpu_directive_5200() {
    let src = b"\t.cpu\t5200\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu.number, 5200);
    assert_ne!(ctx.cpu.features & rhas::options::cpu::C520, 0);
}
