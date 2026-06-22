use super::*;
use crate::addressing::{parse_ea, EffectiveAddress};
use crate::expr::rpn::RPNToken;
use crate::options::cpu;
use crate::symbol::SymbolTable;

fn sym() -> SymbolTable {
    SymbolTable::new(false)
}

fn parse(s: &str) -> EffectiveAddress {
    let t = sym();
    let mut pos = 0;
    parse_ea(s.as_bytes(), &mut pos, &t, cpu::C000).unwrap()
}

fn encode(handler: InsnHandler, opcode: u16, size: SizeCode, ops: Vec<&str>) -> Vec<u8> {
    let operands: Vec<EffectiveAddress> = ops.iter().map(|s| parse(s)).collect();
    encode_insn(opcode, handler, size, &operands).unwrap()
}


// ---- no-operand (NOP / RTS etc.) ----

#[test]
fn test_nop() {
    // NOP: 0x4E71
    let v = encode(InsnHandler::Bcc, 0x4E71, SizeCode::None, vec![]);
    assert_eq!(v, vec![0x4E, 0x71]);
}

#[test]
fn test_rts() {
    let v = encode(InsnHandler::Bcc, 0x4E75, SizeCode::None, vec![]);
    assert_eq!(v, vec![0x4E, 0x75]);
}

#[test]
fn test_rte() {
    let v = encode(InsnHandler::Bcc, 0x4E73, SizeCode::None, vec![]);
    assert_eq!(v, vec![0x4E, 0x73]);
}

#[test]
fn test_illegal() {
    let v = encode(InsnHandler::Bcc, 0x4AFC, SizeCode::None, vec![]);
    assert_eq!(v, vec![0x4A, 0xFC]);
}

// ---- MOVE ----

#[test]
fn test_move_b_dn_dn() {
    // MOVE.B D0, D1 → 0x1200
    let v = encode(InsnHandler::Move, 0x0000, SizeCode::Byte, vec!["d0", "d1"]);
    assert_eq!(v, vec![0x12, 0x00]);
}

#[test]
fn test_move_w_dn_dn() {
    // MOVE.W D0, D1 → 0x3200
    let v = encode(InsnHandler::Move, 0x0000, SizeCode::Word, vec!["d0", "d1"]);
    assert_eq!(v, vec![0x32, 0x00]);
}

#[test]
fn test_move_l_dn_dn() {
    // MOVE.L D0, D1 → 0x2200
    let v = encode(InsnHandler::Move, 0x0000, SizeCode::Long, vec!["d0", "d1"]);
    assert_eq!(v, vec![0x22, 0x00]);
}

#[test]
fn test_move_w_an_dn() {
    // MOVE.W A0, D1 → source=An(0)=0x08, dest=Dn(1)
    // Opcode: 0x3000 | (1<<9) | (0<<6) | 0x08 = 0x3208
    let v = encode(InsnHandler::Move, 0x0000, SizeCode::Word, vec!["a0", "d1"]);
    assert_eq!(v, vec![0x32, 0x08]);
}

#[test]
fn test_move_l_imm_dn() {
    // MOVE.L #$12345678, D0 → 0x203C + 0x12345678
    let v = encode(
        InsnHandler::Move,
        0x0000,
        SizeCode::Long,
        vec!["#$12345678", "d0"],
    );
    assert_eq!(v, vec![0x20, 0x3C, 0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn test_move_w_dspadr_dn() {
    // MOVE.W (4,a1), D2 → 0x3429 + 0x0004
    // src EA: (4,a1) = DSPADR|1 = 0x29
    // dest: D2 bits 11-9=010, mode=000 → bits 11-6 = 010_000 = 0x0400
    let v = encode(
        InsnHandler::Move,
        0x0000,
        SizeCode::Word,
        vec!["(4,a1)", "d2"],
    );
    assert_eq!(v, vec![0x34, 0x29, 0x00, 0x04]);
}

// ---- MOVEA ----

#[test]
fn test_movea_w() {
    // MOVEA.W D0, A1 → 0x3240
    // 0011 001 001 000000 = 0x3240
    let v = encode(InsnHandler::MoveA, 0x2040, SizeCode::Word, vec!["d0", "a1"]);
    assert_eq!(v, vec![0x32, 0x40]);
}

#[test]
fn test_movea_l() {
    // MOVEA.L D0, A1 → 0x2240
    // 0010 001 001 000000 = 0x2240
    let v = encode(InsnHandler::MoveA, 0x2040, SizeCode::Long, vec!["d0", "a1"]);
    assert_eq!(v, vec![0x22, 0x40]);
}

// ---- MOVEQ ----

#[test]
fn test_moveq() {
    // MOVEQ #1, D0 → 0x7001
    let v = encode(InsnHandler::MoveQ, 0x7000, SizeCode::Long, vec!["#1", "d0"]);
    assert_eq!(v, vec![0x70, 0x01]);
}

#[test]
fn test_moveq_negative() {
    // MOVEQ #-1, D0 → 0x70FF
    let v = encode(
        InsnHandler::MoveQ,
        0x7000,
        SizeCode::Long,
        vec!["#-1", "d0"],
    );
    assert_eq!(v, vec![0x70, 0xFF]);
}

#[test]
fn test_moveq_range_error() {
    // 256 は 8 ビットに入らない → エラー
    let operands = vec![parse("#256"), parse("d0")];
    let result = encode_insn(0x7000, InsnHandler::MoveQ, SizeCode::Long, &operands);
    assert!(result.is_err());
}

// ---- ADD/SUB ----

#[test]
fn test_add_b_dn_dn() {
    // ADD.B D0, D1 → 0xD200 (src→dst, direction=0)
    // Actually ADD <ea>,Dn: base=0xD000, sz=00, Dn=1 in 11-9, EA=D0
    // 0xD000 | 0x00 | (1<<9) | 0 = 0xD200
    let v = encode(
        InsnHandler::SubAdd,
        0xD000,
        SizeCode::Byte,
        vec!["d0", "d1"],
    );
    assert_eq!(v, vec![0xD2, 0x00]);
}

#[test]
fn test_add_w_dn_mem() {
    // ADD.W D0, (A1) → dir=1, base=0xD000|0x0100, sz=0x40, D0 in 11-9, (A1)=0x11
    // 0xD000 | 0x0100 | 0x40 | (0<<9) | 0x11 = 0xD151
    let v = encode(
        InsnHandler::SubAdd,
        0xD000,
        SizeCode::Word,
        vec!["d0", "(a1)"],
    );
    assert_eq!(v, vec![0xD1, 0x51]);
}

#[test]
fn test_sub_l_dn_dn() {
    // SUB.L D2, D3 → base=0x9000, dir=0, sz=0x80, D3 in 11-9, D2 EA=0x02
    // 0x9000 | 0x80 | (3<<9) | 2 = 0x9682
    let v = encode(
        InsnHandler::SubAdd,
        0x9000,
        SizeCode::Long,
        vec!["d2", "d3"],
    );
    assert_eq!(v, vec![0x96, 0x82]);
}

// ---- ADDI/SUBI ----

#[test]
fn test_addi_b() {
    // ADDI.B #5, D0 → 0x0600 | 0x00 | 0x00, then #5 padded = 0x0005
    let v = encode(
        InsnHandler::SubAddI,
        0x0600,
        SizeCode::Byte,
        vec!["#5", "d0"],
    );
    assert_eq!(v, vec![0x06, 0x00, 0x00, 0x05]);
}

#[test]
fn test_subi_w() {
    // SUBI.W #$100, D1 → 0x0441, then 0x0100
    let v = encode(
        InsnHandler::SubAddI,
        0x0400,
        SizeCode::Word,
        vec!["#$100", "d1"],
    );
    assert_eq!(v, vec![0x04, 0x41, 0x01, 0x00]);
}

// ---- ADDQ/SUBQ ----

#[test]
fn test_addq_b() {
    // ADDQ.B #4, D0 → 0x5800 | (4<<9) | 0x00 = 0x5880? No wait:
    // base=0x5000, qval=4, sz=0x00, EA=Dn(0)=0 → 0x5000|(4<<9)|0 = 0x5800
    let v = encode(
        InsnHandler::SubAddQ,
        0x5000,
        SizeCode::Byte,
        vec!["#4", "d0"],
    );
    assert_eq!(v, vec![0x58, 0x00]);
}

#[test]
fn test_subq_w() {
    // SUBQ.W #8, D0 → base=0x5100, qval=0 (8→0), sz=0x40, EA=Dn(0)
    // 0x5100 | (0<<9) | 0x40 | 0 = 0x5140
    let v = encode(
        InsnHandler::SubAddQ,
        0x5100,
        SizeCode::Word,
        vec!["#8", "d0"],
    );
    assert_eq!(v, vec![0x51, 0x40]);
}

// ---- CMP ----

#[test]
fn test_cmp_b_dn_dn() {
    // CMP.B D0, D1 → 0xB000|0x00|(1<<9)|0 = 0xB200
    let v = encode(InsnHandler::Cmp, 0xB000, SizeCode::Byte, vec!["d0", "d1"]);
    assert_eq!(v, vec![0xB2, 0x00]);
}

// ---- NEG/NOT/CLR/TST ----

#[test]
fn test_neg_b() {
    // NEG.B D0 → 0x4400 | 0x00 | 0x00 = 0x4400
    let v = encode(InsnHandler::NegNot, 0x4400, SizeCode::Byte, vec!["d0"]);
    assert_eq!(v, vec![0x44, 0x00]);
}

#[test]
fn test_not_w() {
    // NOT.W D3 → 0x4600 | 0x40 | 0x03 = 0x4643
    let v = encode(InsnHandler::NegNot, 0x4600, SizeCode::Word, vec!["d3"]);
    assert_eq!(v, vec![0x46, 0x43]);
}

#[test]
fn test_clr_l() {
    // CLR.L D0 → 0x4200 | 0x80 = 0x4280
    let v = encode(InsnHandler::Clr, 0x4200, SizeCode::Long, vec!["d0"]);
    assert_eq!(v, vec![0x42, 0x80]);
}

#[test]
fn test_tst_w_mem() {
    // TST.W (A0) → 0x4A00 | 0x40 | 0x10 = 0x4A50
    let v = encode(InsnHandler::Tst, 0x4A00, SizeCode::Word, vec!["(a0)"]);
    assert_eq!(v, vec![0x4A, 0x50]);
}

// ---- EXT ----

#[test]
fn test_ext_w() {
    // EXT.W D0 → 0x4880
    let v = encode(InsnHandler::Ext, 0x4880, SizeCode::Word, vec!["d0"]);
    assert_eq!(v, vec![0x48, 0x80]);
}

#[test]
fn test_ext_l() {
    // EXT.L D0 → 0x48C0
    let v = encode(InsnHandler::Ext, 0x4880, SizeCode::Long, vec!["d0"]);
    assert_eq!(v, vec![0x48, 0xC0]);
}

// ---- SWAP ----

#[test]
fn test_swap() {
    // SWAP D3 → 0x4843
    let v = encode(InsnHandler::Swap, 0x4840, SizeCode::Word, vec!["d3"]);
    assert_eq!(v, vec![0x48, 0x43]);
}

// ---- EXG ----

#[test]
fn test_exg_dn_dn() {
    // EXG D0, D1 → 0xC100 | (0<<9) | (0x08<<3)? No:
    // word = 0xC100 | (0<<9) | (0x08<<3) | 1 = 0xC100 | 0x0040 | 1 = 0xC141
    let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["d0", "d1"]);
    assert_eq!(v, vec![0xC1, 0x41]);
}

#[test]
fn test_exg_an_an() {
    // EXG A0, A1 → 0xC100 | (0<<9) | (0x09<<3) | (1) = 0xC100|0x48|1 = 0xC149
    let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["a0", "a1"]);
    assert_eq!(v, vec![0xC1, 0x49]);
}

#[test]
fn test_exg_dn_an() {
    // EXG D0, A1 → 0xC100 | (0<<9) | (0x11<<3) | 1 = 0xC100|0x88|1 = 0xC189
    let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["d0", "a1"]);
    assert_eq!(v, vec![0xC1, 0x89]);
}

// ---- AND/OR/EOR ----

#[test]
fn test_and_b_dn_dn() {
    // AND.B D0, D1 → <ea>,Dn: 0xC000|(1<<9)|0x00|0 = 0xC200
    let v = encode(InsnHandler::OrAnd, 0xC000, SizeCode::Byte, vec!["d0", "d1"]);
    assert_eq!(v, vec![0xC2, 0x00]);
}

#[test]
fn test_or_w_mem_dn() {
    // OR.W (A0), D1 → 0x8000|0x40|(1<<9)|0x10 = 0x8250+0x10? Actually:
    // base=0x8000, dir=0, sz=0x40, D1 in 11-9=(1<<9)=0x0200, EA=(A0)=0x10
    // 0x8000|0x40|0x0200|0x10 = 0x8250
    let v = encode(
        InsnHandler::OrAnd,
        0x8000,
        SizeCode::Word,
        vec!["(a0)", "d1"],
    );
    assert_eq!(v, vec![0x82, 0x50]);
}

#[test]
fn test_eor_l_dn_dn() {
    // EOR.L D0, D1 → 0xB100|0x80|(0<<9)|1 = 0xB181
    let v = encode(InsnHandler::Eor, 0xB100, SizeCode::Long, vec!["d0", "d1"]);
    assert_eq!(v, vec![0xB1, 0x81]);
}

// ---- SHIFT ----

#[test]
fn test_asr_b_imm_dn() {
    // ASR.B #1, D0 → 0xE000|(1<<9)|0x00|0 = 0xE200
    let v = encode(
        InsnHandler::SftRot,
        0xE000,
        SizeCode::Byte,
        vec!["#1", "d0"],
    );
    assert_eq!(v, vec![0xE2, 0x00]);
}

#[test]
fn test_lsl_w_dn_dn() {
    // LSL.W D1, D0 → 0xE108|0x40|0x20|(1<<9)|0 = ?
    // base=0xE108, &0xFFF8=0xE108, sz=0x40, bit5=0x20, D1=1<<9=0x200, D0=0
    // 0xE108 | 0x40 | 0x20 | 0x0200 = 0xE368
    let v = encode(
        InsnHandler::SftRot,
        0xE108,
        SizeCode::Word,
        vec!["d1", "d0"],
    );
    assert_eq!(v, vec![0xE3, 0x68]);
}

#[test]
fn test_ror_w_imm8_dn() {
    // ROR.W #8, D0 → 0xE018|(8→0)|0x40|0 = 0xE018|0x40|0 = 0xE058
    let v = encode(
        InsnHandler::SftRot,
        0xE018,
        SizeCode::Word,
        vec!["#8", "d0"],
    );
    assert_eq!(v, vec![0xE0, 0x58]);
}

// ---- LEA ----

#[test]
fn test_lea() {
    // LEA (A0), A1 → 0x41C0 | (1<<9) | 0x10 = 0x43D0
    let v = encode(InsnHandler::Lea, 0x41C0, SizeCode::Long, vec!["(a0)", "a1"]);
    assert_eq!(v, vec![0x43, 0xD0]);
}

// ---- PEA ----

#[test]
fn test_pea() {
    // PEA (A0) → 0x4840 | 0x10 = 0x4850
    let v = encode(InsnHandler::PeaJsrJmp, 0x4840, SizeCode::Long, vec!["(a0)"]);
    assert_eq!(v, vec![0x48, 0x50]);
}

// ---- JMP/JSR ----

#[test]
fn test_jsr() {
    // JSR (A0) → 0x4E80 | 0x10 = 0x4E90
    let v = encode(InsnHandler::JmpJsr, 0x4E80, SizeCode::None, vec!["(a0)"]);
    assert_eq!(v, vec![0x4E, 0x90]);
}

#[test]
fn test_jmp_abs() {
    // JMP $1234.w → 0x4EC0 | 0x38 = 0x4EF8, then 0x1234
    let v = encode(InsnHandler::JmpJsr, 0x4EC0, SizeCode::None, vec!["$1234.w"]);
    assert_eq!(v, vec![0x4E, 0xF8, 0x12, 0x34]);
}

// ---- ADDQ / SUBQ edge cases ----

#[test]
fn test_addq_8() {
    // ADDQ.W #8, D0 → qval=0, sz=0x40, base=0x5000
    // 0x5000|(0<<9)|0x40|0 = 0x5040
    let v = encode(
        InsnHandler::SubAddQ,
        0x5000,
        SizeCode::Word,
        vec!["#8", "d0"],
    );
    assert_eq!(v, vec![0x50, 0x40]);
}

// ---- BTST / BSET ----

#[test]
fn test_btst_static() {
    // BTST #3, D0 → 0x0000|0x0800|0x00, then 0x0003
    let v = encode(InsnHandler::Btst, 0x0000, SizeCode::None, vec!["#3", "d0"]);
    assert_eq!(v, vec![0x08, 0x00, 0x00, 0x03]);
}

#[test]
fn test_btst_dynamic() {
    // BTST D0, D1 → 0x0000|0x0100|(0<<9)|1 = 0x0101
    let v = encode(InsnHandler::Btst, 0x0000, SizeCode::None, vec!["d0", "d1"]);
    assert_eq!(v, vec![0x01, 0x01]);
}

#[test]
fn test_bset_static() {
    // BSET #7, D3 → 0x00C0|0x0800|3, then 0x0007
    let v = encode(
        InsnHandler::BchClSt,
        0x00C0,
        SizeCode::None,
        vec!["#7", "d3"],
    );
    assert_eq!(v, vec![0x08, 0xC3, 0x00, 0x07]);
}

// ---- TRAP ----

#[test]
fn test_trap() {
    // TRAP #1 → 0x4E41
    let v = encode(InsnHandler::Trap, 0x4E40, SizeCode::None, vec!["#1"]);
    assert_eq!(v, vec![0x4E, 0x41]);
}

// ---- STOP ----

#[test]
fn test_stop() {
    // STOP #$2700 → 0x4E72, 0x2700
    let v = encode(InsnHandler::StopRtd, 0x4E72, SizeCode::None, vec!["#$2700"]);
    assert_eq!(v, vec![0x4E, 0x72, 0x27, 0x00]);
}

// ---- UNLK ----

#[test]
fn test_unlk() {
    // UNLK A0 → 0x4E58
    let v = encode(InsnHandler::Unlk, 0x4E58, SizeCode::None, vec!["a0"]);
    assert_eq!(v, vec![0x4E, 0x58]);
}

// ---- ADDA/SUBA/CMPA ----

#[test]
fn test_adda_w() {
    // ADDA.W D0, A1 → 0xD0C0|(1<<9)|0 = 0xD2C0? No wait:
    // base=0xD0C0, size_bit=0 (word), An=1 in 11-9, src=D0=0
    // 0xD0C0|(1<<9)|0 = 0xD2C0
    let v = encode(
        InsnHandler::SbAdCpA,
        0xD0C0,
        SizeCode::Word,
        vec!["d0", "a1"],
    );
    assert_eq!(v, vec![0xD2, 0xC0]);
}

#[test]
fn test_adda_l() {
    // ADDA.L D0, A1 → 0xD0C0|0x100|(1<<9)|0 = 0xD3C0
    let v = encode(
        InsnHandler::SbAdCpA,
        0xD0C0,
        SizeCode::Long,
        vec!["d0", "a1"],
    );
    assert_eq!(v, vec![0xD3, 0xC0]);
}

// ---- ADDX/SUBX ----

#[test]
fn test_addx_dn() {
    // ADDX.B D0, D1 → 0xD100|0x00|(1<<9)|0 = 0xD300
    let v = encode(
        InsnHandler::SubAddX,
        0xD100,
        SizeCode::Byte,
        vec!["d0", "d1"],
    );
    assert_eq!(v, vec![0xD3, 0x00]);
}

#[test]
fn test_subx_predec() {
    // SUBX.W -(A0), -(A1) → 0x9100|0x40|0x08|(1<<9)|0 = ?
    // base=0x9100, sz=0x40, mode=0x08, Ax=1 in 11-9, Ay=0
    // 0x9100|0x40|0x08|0x0200|0 = 0x9348
    let v = encode(
        InsnHandler::SubAddX,
        0x9100,
        SizeCode::Word,
        vec!["-(a0)", "-(a1)"],
    );
    assert_eq!(v, vec![0x93, 0x48]);
}

// ---- Scc ----

#[test]
fn test_st() {
    // ST D0 → 0x50C0 | 0x00 = 0x50C0
    let v = encode(InsnHandler::Scc, 0x50C0, SizeCode::Byte, vec!["d0"]);
    assert_eq!(v, vec![0x50, 0xC0]);
}

#[test]
fn test_sne() {
    // SNE (A0) → 0x56C0 | 0x10 = 0x56D0
    let v = encode(InsnHandler::Scc, 0x56C0, SizeCode::Byte, vec!["(a0)"]);
    assert_eq!(v, vec![0x56, 0xD0]);
}

// ---- DEC/INC ----

#[test]
fn test_dec_b() {
    // DEC.B D0 → SUBQ #1, D0 = 0x5300|(1<<9)|0x00|0 = 0x5500
    let v = encode(InsnHandler::DecInc, 0x5300, SizeCode::Byte, vec!["d0"]);
    assert_eq!(v, vec![0x53, 0x00]);
}

// ---- EXG variant ----

#[test]
fn test_exg_an_dn() {
    // EXG A0, D1 (same as EXG D1, A0) → mode=0x11, Rx=D1=1, Ry=A0=0
    // 0xC100|(1<<9)|(0x11<<3)|0 = 0xC100|0x0200|0x0088 = 0xC388
    let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["a0", "d1"]);
    // EXG An, Dn: rx=Dn, ry=An (swap)
    // For (A0, D1): operands[0]=A0 (An), operands[1]=D1 (Dn) → mode=0x11, rx=D1=1, ry=A0=0
    assert_eq!(v, vec![0xC3, 0x88]);
}

// ---- Branch DeferToLinker ----

#[test]
fn test_bra_defers() {
    let operands = vec![parse("$1000")];
    let result = encode_insn(0x6000, InsnHandler::Bcc, SizeCode::Word, &operands);
    assert_eq!(result, Err(InsnError::DeferToLinker));
}

// ---- MULU/DIVS ----

#[test]
fn test_mulu_w() {
    // MULU.W D0, D1 → 0xC0C0|(1<<9)|0 = 0xC2C0
    let v = encode(
        InsnHandler::DivMul,
        0xC0C0,
        SizeCode::Word,
        vec!["d0", "d1"],
    );
    assert_eq!(v, vec![0xC2, 0xC0]);
}

#[test]
fn test_divs_w() {
    // DIVS.W D0, D1 → 0x81C0|(1<<9)|0 = 0x83C0
    let v = encode(
        InsnHandler::DivMul,
        0x81C0,
        SizeCode::Word,
        vec!["d0", "d1"],
    );
    assert_eq!(v, vec![0x83, 0xC0]);
}

// ---- ABCD/SBCD ----

#[test]
fn test_abcd_dn() {
    // ABCD D0, D1 → 0xC100|(1<<9)|0x00|0 = 0xC300
    let v = encode(InsnHandler::SAbcd, 0xC100, SizeCode::Byte, vec!["d0", "d1"]);
    assert_eq!(v, vec![0xC3, 0x00]);
}

#[test]
fn test_sbcd_predec() {
    // SBCD -(A0), -(A1) → 0x8100|(1<<9)|0x08|0 = 0x8308
    let v = encode(
        InsnHandler::SAbcd,
        0x8100,
        SizeCode::Byte,
        vec!["-(a0)", "-(a1)"],
    );
    assert_eq!(v, vec![0x83, 0x08]);
}

// ---- CMPM ----

#[test]
fn test_cmpm_w() {
    // CMPM.W (A0)+, (A1)+ → 0xB108|0x40|(1<<9)|0 = 0xB348
    let v = encode(
        InsnHandler::CmpM,
        0xB108,
        SizeCode::Word,
        vec!["(a0)+", "(a1)+"],
    );
    assert_eq!(v, vec![0xB3, 0x48]);
}

// ---- MOVEM ----

#[test]
fn test_movem_to_mem() {
    // MOVEM.W #0x00FF, (A0)
    // reg→mem: direction=0, sz=0, EA=(A0)=0x10, mask=0x00FF
    // opcode = 0x4880|0|0|0x10 = 0x4890, then mask=0x00FF
    let operands = vec![
        EffectiveAddress::Immediate(vec![RPNToken::Value(0x00FF)]),
        parse("(a0)"),
    ];
    let result = encode_insn(0x4880, InsnHandler::MoveM, SizeCode::Word, &operands);
    assert!(result.is_ok());
    let v = result.unwrap();
    assert_eq!(v[0..2], [0x48, 0x90]);
    assert_eq!(v[2..4], [0x00, 0xFF]);
}
