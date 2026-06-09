use super::P1Ctx;
use crate::addressing::EffectiveAddress;
use crate::expr::rpn::RPNToken;
use crate::symbol::types::{InsnHandler, SizeCode};

pub(crate) fn optimize_instruction(
    handler: InsnHandler,
    opcode: u16,
    size: SizeCode,
    mut ops: Vec<EffectiveAddress>,
    p1: &mut P1Ctx<'_>,
) -> Option<(InsnHandler, u16, SizeCode, Vec<EffectiveAddress>)> {
    let mut handler = handler;
    let mut opcode = opcode;
    let mut enc_size = size;

    if matches!(
        handler,
        InsnHandler::FMove
            | InsnHandler::FMoveM
            | InsnHandler::FMoveCr
            | InsnHandler::FSinCos
            | InsnHandler::FArith
            | InsnHandler::FCmp
            | InsnHandler::FTst
            | InsnHandler::FNop
            | InsnHandler::FSave
            | InsnHandler::FRestore
            | InsnHandler::FBcc
            | InsnHandler::FDBcc
    ) {
        opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
    }

    // MOVE.l #imm,Dn → MOVEQ #imm,Dn（#-128..255）
    // MOVE.b/.w #0,Dn → CLR.b/.w Dn
    if matches!(handler, InsnHandler::Move) && ops.len() >= 2 {
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(_)) = (&ops[0], &ops[1])
        {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 {
                    if enc_size == SizeCode::Long
                        && !p1.ctx.opts.no_quick
                        && ev.value >= -128
                        && ev.value <= 127
                    {
                        handler = InsnHandler::MoveQ;
                        opcode = 0x7000;
                    } else if p1.ctx.opts.opt_move0
                        && ev.value == 0
                        && matches!(enc_size, SizeCode::Byte | SizeCode::Word)
                    {
                        handler = InsnHandler::Clr;
                        opcode = 0x4200;
                        ops = vec![ops[1].clone()];
                    }
                }
            }
        }
    }

    // CLR.l Dn → MOVEQ #0,Dn（68000/68010のみ）
    if matches!(handler, InsnHandler::Clr)
        && p1.ctx.opts.opt_clr
        && enc_size == SizeCode::Long
        && p1.ctx.cpu.number < 68020
        && ops.len() == 1
        && matches!(ops[0], EffectiveAddress::DataReg(_))
    {
        handler = InsnHandler::MoveQ;
        opcode = 0x7000;
        ops = vec![
            EffectiveAddress::Immediate(vec![RPNToken::Value(0), RPNToken::End]),
            ops[0].clone(),
        ];
    }

    // CMP #0,Dn → TST Dn
    if matches!(handler, InsnHandler::Cmp)
        && p1.ctx.opts.opt_cmp0
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::DataReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // CMPI #0,<ea> → TST <ea>
    if matches!(handler, InsnHandler::CmpI) && p1.ctx.opts.opt_cmpi0 && ops.len() == 2 {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // SUBI/ADDI #imm(1-8),<ea> → SUBQ/ADDQ
    if matches!(handler, InsnHandler::SubAddI) && !p1.ctx.opts.no_quick && ops.len() >= 2 {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= 1 && ev.value <= 8 {
                    handler = InsnHandler::SubAddQ;
                    opcode = if (opcode & 0x0200) != 0 {
                        0x5000
                    } else {
                        0x5100
                    };
                }
            }
        }
    }

    // ADD/SUB #imm(1-8), <ea> → ADDQ/SUBQ
    if matches!(handler, InsnHandler::SubAdd) && !p1.ctx.opts.no_quick && ops.len() >= 2 {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= 1 && ev.value <= 8 {
                    handler = InsnHandler::SubAddQ;
                    opcode = if opcode & 0x4000 != 0 { 0x5000 } else { 0x5100 };
                }
            }
        }
    }

    // MOVEA.L #d16,An → MOVEA.W #d16,An
    if matches!(handler, InsnHandler::MoveA)
        && p1.ctx.opts.opt_movea
        && enc_size == SizeCode::Long
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= -32768 && ev.value <= 32767 {
                    enc_size = SizeCode::Word;
                }
            }
        }
    }

    // CMPA #0,An → TST.L An（68020+）
    if matches!(handler, InsnHandler::CmpA)
        && p1.ctx.opts.opt_cmpa
        && enc_size == SizeCode::Long
        && p1.ctx.cpu.number >= 68020
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 0 {
                    handler = InsnHandler::Tst;
                    opcode = 0x4A00;
                    ops = vec![ops[1].clone()];
                }
            }
        }
    }

    // CMPA.L #d16,An → CMPA.W #d16,An
    if matches!(handler, InsnHandler::CmpA)
        && p1.ctx.opts.opt_cmpa
        && enc_size == SizeCode::Long
        && ops.len() == 2
        && matches!(ops[1], EffectiveAddress::AddrReg(_))
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= -32768 && ev.value <= 32767 {
                    enc_size = SizeCode::Word;
                }
            }
        }
    }

    // LEA 最適化:
    //   LEA (An),An / LEA (0,An),An → 削除
    //   LEA (d,An),An (d=-8..-1,1..8) → SUBQ/ADDQ.W #|d|,An
    if matches!(handler, InsnHandler::Lea) && p1.ctx.opts.opt_lea && ops.len() == 2 {
        if let (src, EffectiveAddress::AddrReg(dst_an)) = (&ops[0], &ops[1]) {
            match src {
                EffectiveAddress::AddrRegInd(src_an) if src_an == dst_an => {
                    return None;
                }
                EffectiveAddress::AddrRegDisp { an: src_an, disp } if src_an == dst_an => {
                    let disp_const = disp.const_val.or_else(|| {
                        p1.eval_const(&disp.rpn).and_then(|ev| {
                            if ev.section == 0 {
                                Some(ev.value)
                            } else {
                                None
                            }
                        })
                    });
                    if let Some(d) = disp_const {
                        if d == 0 {
                            return None;
                        }
                        if (1..=8).contains(&d) || (-8..=-1).contains(&d) {
                            handler = InsnHandler::SubAddQ;
                            opcode = if d > 0 { 0x5000 } else { 0x5100 };
                            enc_size = SizeCode::Word;
                            let imm = if d > 0 { d } else { -d };
                            ops = vec![
                                EffectiveAddress::Immediate(vec![
                                    RPNToken::Value(imm as u32),
                                    RPNToken::End,
                                ]),
                                EffectiveAddress::AddrReg(*dst_an),
                            ];
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ASL #1,Dn → ADD Dn,Dn（68060以外）
    if matches!(handler, InsnHandler::Asl)
        && p1.ctx.opts.opt_asl
        && p1.ctx.cpu.number < 68060
        && ops.len() == 2
    {
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(dn)) =
            (&ops[0], &ops[1])
        {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 1 {
                    handler = InsnHandler::SubAdd;
                    opcode = 0xD000; // ADD
                    ops = vec![
                        EffectiveAddress::DataReg(*dn),
                        EffectiveAddress::DataReg(*dn),
                    ];
                }
            }
        }
    }

    // no_null_disp: displacement=0 の (An) 形式への最適化を抑制
    if p1.ctx.opts.no_null_disp {
        for ea in &mut ops {
            if let EffectiveAddress::AddrRegDisp { disp, .. } = ea {
                if disp.size.is_none() && disp.const_val == Some(0) {
                    disp.size = Some(crate::addressing::DispSize::Word);
                } else if disp.size.is_none() && disp.const_val.is_none() {
                    // const_val 未設定でも rpn が定数 0 なら size を設定
                    if let Some(ev) = p1.eval_const(&disp.rpn) {
                        if ev.section == 0 && ev.value == 0 {
                            disp.size = Some(crate::addressing::DispSize::Word);
                        }
                    }
                }
            }
        }
    }

    Some((handler, opcode, enc_size, ops))
}
