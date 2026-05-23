use crate::addressing::EffectiveAddress;
use crate::error::ErrorCode;
use crate::expr::{eval_rpn, Rpn, parse_expr};
use crate::expr::eval::EvalValue;
use crate::expr::rpn::RPNToken;
use crate::instructions::{encode_insn, InsnError};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, InsnHandler, SizeCode, reg};
use crate::pass::temp::TempRecord;
use super::P1Ctx;
use super::operand::parse_operands;
use super::skip_spaces;

pub(super) fn handle_real_insn(
    handler:  InsnHandler,
    opcode:   u16,
    size:     Option<SizeCode>,
    line:     &[u8],
    pos:      usize,
    records:  &mut Vec<TempRecord>,
    p1:       &mut P1Ctx<'_>,
) {
    // HAS のデフォルトサイズはワード（サフィックスなし → .w 相当）
    let sz = size.unwrap_or(SizeCode::Word);
    let cpu = p1.cpu_type();

    // 分岐命令（ターゲットを RPN として保持）
    if matches!(handler, InsnHandler::Bcc | InsnHandler::JBcc) {
        let target = parse_branch_target(line, pos);
        if let Some(rpn) = target {
            let byte_sz = crate::pass::temp::branch_word_size(size);
            p1.advance(byte_sz);
            records.push(TempRecord::Branch {
                opcode,
                target: rpn,
                req_size: size,
                cur_size: size,
                suppressed: false,
            });
        } else {
            // オペランドなし (NOP/RTS 等)
            if let Ok(bytes) = encode_insn(opcode, handler, sz, &[]) {
                p1.advance(bytes.len() as u32);
                records.push(TempRecord::Const(bytes));
            }
        }
        return;
    }

    // FBcc: ターゲットを RPN として保持（Pass3 でPC相対計算）
    if matches!(handler, InsnHandler::FBcc) {
        if let Some(rpn) = parse_branch_target(line, pos) {
            let opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
            let req = size.unwrap_or(SizeCode::None);
            let byte_size = match req {
                SizeCode::Long => 6,
                _ => 4, // .w または自動
            };
            p1.advance(byte_size);
            records.push(TempRecord::DeferredInsn {
                base: opcode,
                handler,
                size: req,
                ops: vec![EffectiveAddress::AbsLong(rpn)],
                byte_size,
            });
        }
        return;
    }

    // DBcc: ターゲットを RPN として保持
    if matches!(handler, InsnHandler::DBcc) {
        let mut ops = parse_operands(line, pos, p1.sym, cpu);
        if ops.len() == 2 {
            // ops[0] = Dn, ops[1] = label (as AbsLong RPN)
            let dn = ops.remove(0);
            let target = if let EffectiveAddress::AbsLong(rpn) = ops.remove(0) {
                rpn
            } else {
                vec![RPNToken::Value(0), RPNToken::End]
            };
            // estimate 4 bytes: opcode(2) + dn + offset(2)
            // Actually DBcc is 2(opcode) + 2(offset) = 4 bytes, Dn is encoded in opcode
            p1.advance(4);
            records.push(TempRecord::DeferredInsn {
                base: opcode, handler, size: sz,
                ops: vec![dn, EffectiveAddress::AbsLong(target)],
                byte_size: 4,
            });
        }
        return;
    }

    // FDBcc: Dn,ターゲットを保持（Pass3 でPC相対計算）
    if matches!(handler, InsnHandler::FDBcc) {
        let mut ops = parse_operands(line, pos, p1.sym, cpu);
        if ops.len() == 2 {
            let opcode = (opcode & !0x0E00) | ((u16::from(p1.ctx.fpid & 0x07)) << 9);
            let dn = ops.remove(0);
            let target = if let EffectiveAddress::AbsLong(rpn) = ops.remove(0) {
                rpn
            } else {
                vec![RPNToken::Value(0), RPNToken::End]
            };
            p1.advance(6);
            records.push(TempRecord::DeferredInsn {
                base: opcode,
                handler,
                size: SizeCode::None,
                ops: vec![dn, EffectiveAddress::AbsLong(target)],
                byte_size: 6,
            });
        }
        return;
    }

    // 通常命令
    let mut ops = parse_operands(line, pos, &*p1.sym, cpu);
    let mut enc_size = sz;

    // JMP/JSR 最適化（安全に判定できるケースのみ）
    if matches!(handler, InsnHandler::JmpJsr) && p1.ctx.opts.opt_jmp_jsr && ops.len() == 1 {
        match &ops[0] {
            // jmp/jsr (2,pc): jmpは削除、jsrはpea (2,pc)
            EffectiveAddress::PcDisp(disp) if disp.size.is_none() && disp.const_val == Some(2) => {
                if opcode == 0x4EC0 {
                    // jmp (2,pc) は命令自体を削除
                    return;
                }
                if opcode == 0x4E80 {
                    // jsr (2,pc) → pea (2,pc)
                    let bytes = vec![0x48, 0x7A, 0x00, 0x02];
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                    return;
                }
            }
            // jmp/jsr label（サイズ指定なし）→ jbra/jbsr 相当の分岐最適化パスへ渡す
            // オリジナルは定数ターゲット（jmp $FF0038 など）を除いて変換する。
            EffectiveAddress::AbsLong(rpn) if !single_operand_has_explicit_long_suffix(line, pos) => {
                let is_const_abs = matches!(p1.eval_const(rpn), Some(v) if v.section == 0);
                if !is_const_abs {
                    let bcc_opcode = if opcode == 0x4E80 { 0x6100 } else { 0x6000 };
                    let byte_sz = crate::pass::temp::branch_word_size(None);
                    p1.advance(byte_sz);
                    records.push(TempRecord::Branch {
                        opcode: bcc_opcode,
                        target: rpn.clone(),
                        req_size: None,
                        cur_size: None,
                        suppressed: false,
                    });
                    return;
                }
            }
            // jmp/jsr (label,pc)（サイズ指定なし・非定数）も分岐最適化へ渡す
            EffectiveAddress::PcDisp(disp)
                if disp.size.is_none() && disp.const_val.is_none() =>
            {
                let bcc_opcode = if opcode == 0x4E80 { 0x6100 } else { 0x6000 };
                let byte_sz = crate::pass::temp::branch_word_size(None);
                p1.advance(byte_sz);
                records.push(TempRecord::Branch {
                    opcode: bcc_opcode,
                    target: disp.rpn.clone(),
                    req_size: None,
                    cur_size: None,
                    suppressed: false,
                });
                return;
            }
            _ => {}
        }
    }

    // 命令最適化（-c4）
    let mut handler = handler;
    let mut opcode = opcode;
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
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(_)) = (&ops[0], &ops[1]) {
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
    if matches!(handler, InsnHandler::CmpI)
        && p1.ctx.opts.opt_cmpi0
        && ops.len() == 2
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

    // SUBI/ADDI #imm(1-8),<ea> → SUBQ/ADDQ
    if matches!(handler, InsnHandler::SubAddI)
        && !p1.ctx.opts.no_quick
        && ops.len() >= 2
    {
        if let EffectiveAddress::Immediate(rpn) = &ops[0] {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value >= 1 && ev.value <= 8 {
                    handler = InsnHandler::SubAddQ;
                    opcode = if (opcode & 0x0200) != 0 { 0x5000 } else { 0x5100 };
                }
            }
        }
    }

    // ADD/SUB #imm(1-8), <ea> → ADDQ/SUBQ
    if matches!(handler, InsnHandler::SubAdd)
        && !p1.ctx.opts.no_quick
        && ops.len() >= 2
    {
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
    if matches!(handler, InsnHandler::Lea)
        && p1.ctx.opts.opt_lea
        && ops.len() == 2
    {
        if let (src, EffectiveAddress::AddrReg(dst_an)) = (&ops[0], &ops[1]) {
            match src {
                EffectiveAddress::AddrRegInd(src_an) if src_an == dst_an => {
                    return;
                }
                EffectiveAddress::AddrRegDisp { an: src_an, disp }
                    if src_an == dst_an =>
                {
                    let disp_const = disp.const_val.or_else(|| {
                        p1.eval_const(&disp.rpn)
                            .and_then(|ev| if ev.section == 0 { Some(ev.value) } else { None })
                    });
                    if let Some(d) = disp_const {
                        if d == 0 {
                            return;
                        }
                        if (1..=8).contains(&d) || (-8..=-1).contains(&d) {
                            handler = InsnHandler::SubAddQ;
                            opcode = if d > 0 { 0x5000 } else { 0x5100 };
                            enc_size = SizeCode::Word;
                            let imm = if d > 0 { d } else { -d };
                            ops = vec![
                                EffectiveAddress::Immediate(vec![RPNToken::Value(imm as u32), RPNToken::End]),
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
        if let (EffectiveAddress::Immediate(rpn), EffectiveAddress::DataReg(dn)) = (&ops[0], &ops[1]) {
            if let Some(ev) = p1.eval_const(rpn) {
                if ev.section == 0 && ev.value == 1 {
                    handler = InsnHandler::SubAdd;
                    opcode = 0xD000; // ADD
                    ops = vec![EffectiveAddress::DataReg(*dn), EffectiveAddress::DataReg(*dn)];
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

    match encode_insn(opcode, handler, enc_size, &ops) {
        Ok(bytes) => {
            p1.advance(bytes.len() as u32);
            records.push(TempRecord::Const(bytes));
        }
        Err(InsnError::DeferToLinker) => {
            // シンボル参照あり → 現時点で定数解決できるものは確定する
            // （.set の時系列値を保持するため）。未確定は Pass3 に延期。
            let can_freeze_now = ops.iter().all(|ea| !ea_has_dynamic_ref(ea, p1.sym));
            if can_freeze_now {
                let resolved: Vec<EffectiveAddress> = ops.iter()
                    .map(|ea| resolve_ea_const_for_size(ea, p1.sym, p1.ctx.opts.no_null_disp))
                    .collect();
                match encode_insn(opcode, handler, enc_size, &resolved) {
                    Ok(bytes) => {
                        p1.advance(bytes.len() as u32);
                        records.push(TempRecord::Const(bytes));
                    }
                    Err(_) => {
                        let byte_size = estimate_insn_size(opcode, handler, enc_size, &ops);
                        p1.advance(byte_size);
                        records.push(TempRecord::DeferredInsn {
                            base: opcode, handler, size: enc_size, ops, byte_size,
                        });
                    }
                }
            } else {
                let byte_size = estimate_insn_size(opcode, handler, enc_size, &ops);
                p1.advance(byte_size);
                records.push(TempRecord::DeferredInsn {
                    base: opcode, handler, size: enc_size, ops, byte_size,
                });
            }
        }
        Err(_) => {
            p1.error_code(ErrorCode::Expr, None);
        }
    }
}

fn single_operand_has_explicit_long_suffix(line: &[u8], pos: usize) -> bool {
    let mut end = line.len();
    if let Some(i) = line[pos..].iter().position(|&b| b == b';') {
        end = pos + i;
    }
    let mut s = &line[pos..end];
    while !s.is_empty() && matches!(s[0], b' ' | b'\t') { s = &s[1..]; }
    while !s.is_empty() && matches!(s[s.len() - 1], b' ' | b'\t') { s = &s[..s.len() - 1]; }
    let sl = crate::utils::to_lowercase_vec(s);
    sl.ends_with(b".l")
}

/// 分岐命令のターゲット RPN を解析する
/// オペランドがなければ None を返す（NOP/RTS 等）
fn parse_branch_target(line: &[u8], mut pos: usize) -> Option<Rpn> {
    skip_spaces(line, &mut pos);
    if pos >= line.len() || line[pos] == b';' {
        return None; // no operand
    }
    let mut p = pos;
    parse_expr(line, &mut p).ok()
}

/// 命令の推定バイト数（シンボル参照を 0 に置換してエンコード）
fn estimate_insn_size(
    base: u16, handler: InsnHandler, size: SizeCode, ops: &[EffectiveAddress]
) -> u32 {
    let placeholder: Vec<EffectiveAddress> =
        ops.iter().map(placeholder_ea).collect();
    match encode_insn(base, handler, size, &placeholder) {
        Ok(bytes) => bytes.len() as u32,
        Err(_) => {
            // フォールバック: EA 拡張ワードサイズの和
            2 + ops.iter().map(ea_ext_words).sum::<u32>()
        }
    }
}

/// EA 内の RPN を pass1 シンボルテーブルで解決して定数に置換する（サイズ推定精度向上のため）
fn resolve_ea_const_for_size(ea: &EffectiveAddress, sym: &SymbolTable, no_null_disp: bool) -> EffectiveAddress {
    use crate::addressing::Displacement;
    let lookup = |name: &[u8]| -> Option<EvalValue> {
        sym.lookup_sym(name).and_then(|s| {
            if let Symbol::Value { value, section, .. } = s {
                Some(EvalValue { value: *value, section: *section })
            } else { None }
        })
    };
    match ea {
        EffectiveAddress::Immediate(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::Immediate(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AbsLong(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::AbsShort(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AbsShort(rpn) => {
            if let Ok(v) = eval_rpn(rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    return EffectiveAddress::AbsShort(
                        vec![RPNToken::Value(v.value as u32), RPNToken::End]);
                }
            }
            ea.clone()
        }
        EffectiveAddress::AddrRegDisp { an, disp } => {
            if let Ok(v) = eval_rpn(&disp.rpn, 0, 0, 0, &lookup) {
                if v.section == 0 {
                    // no_null_disp: displacement=0 の最適化を抑制するため明示的サイズを設定
                    let size = if no_null_disp && v.value == 0 && disp.size.is_none() {
                        Some(crate::addressing::DispSize::Word)
                    } else {
                        disp.size
                    };
                    return EffectiveAddress::AddrRegDisp {
                        an: *an,
                        disp: Displacement {
                            rpn: vec![RPNToken::Value(v.value as u32), RPNToken::End],
                            size,
                            const_val: Some(v.value),
                        },
                    };
                }
            }
            ea.clone()
        }
        _ => ea.clone(),
    }
}

fn ea_has_dynamic_ref(ea: &EffectiveAddress, sym: &SymbolTable) -> bool {
    match ea {
        EffectiveAddress::Immediate(rpn)
        | EffectiveAddress::AbsShort(rpn)
        | EffectiveAddress::AbsLong(rpn) => rpn_has_dynamic_ref(rpn, sym),
        EffectiveAddress::AddrRegDisp { disp, .. }
        | EffectiveAddress::PcDisp(disp) => rpn_has_dynamic_ref(&disp.rpn, sym),
        EffectiveAddress::AddrRegIdx { disp, .. }
        | EffectiveAddress::PcIdx { disp, .. } => rpn_has_dynamic_ref(&disp.rpn, sym),
        EffectiveAddress::MemIndPost { bd, od, .. }
        | EffectiveAddress::MemIndPre { bd, od, .. }
        | EffectiveAddress::PcMemIndPost { bd, od, .. }
        | EffectiveAddress::PcMemIndPre { bd, od, .. } => {
            rpn_has_dynamic_ref(&bd.rpn, sym) || rpn_has_dynamic_ref(&od.rpn, sym)
        }
        _ => false,
    }
}

fn rpn_has_dynamic_ref(rpn: &Rpn, sym: &SymbolTable) -> bool {
    for tok in rpn {
        match tok {
            RPNToken::Location | RPNToken::CurrentLoc => return true,
            RPNToken::SymbolRef(name) => {
                match sym.lookup_sym(name) {
                    Some(Symbol::Value { section, attrib, .. }) => {
                        if *attrib < DefAttrib::Define || *section != 0 {
                            return true;
                        }
                    }
                    _ => return true,
                }
            }
            _ => {}
        }
    }
    false
}

/// EA の拡張ワードバイト数（おおよその見積もり）
fn ea_ext_words(ea: &EffectiveAddress) -> u32 {
    match ea {
        EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_)
        | EffectiveAddress::AddrRegInd(_) | EffectiveAddress::AddrRegPostInc(_)
        | EffectiveAddress::AddrRegPreDec(_) => 0,
        EffectiveAddress::AbsShort(_) | EffectiveAddress::AddrRegDisp { .. }
        | EffectiveAddress::PcDisp(_) => 2,
        EffectiveAddress::AbsLong(_) => 4,
        EffectiveAddress::Immediate(rpn) => {
            // デフォルト: ワード
            let _ = rpn;
            2
        }
        EffectiveAddress::AddrRegIdx { .. } | EffectiveAddress::PcIdx { .. } => 2,
        EffectiveAddress::MemIndPost { .. } | EffectiveAddress::MemIndPre { .. }
        | EffectiveAddress::PcMemIndPost { .. } | EffectiveAddress::PcMemIndPre { .. } => 6,
        EffectiveAddress::CcrReg | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_) | EffectiveAddress::FpCtrlReg(_) => 0,
    }
}

/// EA 内のシンボル参照を定数に置換したコピーを返す（命令バイト数推定用）
fn placeholder_ea(ea: &EffectiveAddress) -> EffectiveAddress {
    use crate::addressing::Displacement;
    // 即値は 1 を使う。0 だと SUBQ/ADDQ の範囲チェック (1-8) に引っかかるため。
    let one_rpn = || vec![RPNToken::Value(1), RPNToken::End];
    let zero_rpn = || vec![RPNToken::Value(0), RPNToken::End];
    match ea {
        EffectiveAddress::Immediate(_) => EffectiveAddress::Immediate(one_rpn()),
        EffectiveAddress::AbsShort(_)  => EffectiveAddress::AbsShort(zero_rpn()),
        EffectiveAddress::AbsLong(_)   => EffectiveAddress::AbsLong(zero_rpn()),
        EffectiveAddress::AddrRegDisp { an, disp } if disp.const_val.is_none() => {
            // ディスプレースメントが未確定（外部参照など）の場合、非ゼロのプレースホルダーを使用。
            // ゼロを使うと (0,An)→(An) 最適化が誤って適用されてしまうため。
            EffectiveAddress::AddrRegDisp {
                an: *an,
                disp: Displacement { rpn: one_rpn(), size: disp.size, const_val: Some(1) },
            }
        }
        EffectiveAddress::PcDisp(disp) if disp.const_val.is_none() => {
            EffectiveAddress::PcDisp(
                Displacement { rpn: one_rpn(), size: disp.size, const_val: Some(1) }
            )
        }
        EffectiveAddress::MemIndPost { an, bd, idx, od } => {
            EffectiveAddress::MemIndPost {
                an: *an, idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::MemIndPre { an, bd, idx, od } => {
            EffectiveAddress::MemIndPre {
                an: *an, idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::PcMemIndPost { bd, idx, od } => {
            EffectiveAddress::PcMemIndPost {
                idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        EffectiveAddress::PcMemIndPre { bd, idx, od } => {
            EffectiveAddress::PcMemIndPre {
                idx: idx.clone(),
                bd: Displacement { rpn: if bd.const_val.is_some() { bd.rpn.clone() } else { one_rpn() }, size: bd.size, const_val: bd.const_val.or(Some(1)) },
                od: Displacement { rpn: if od.const_val.is_some() { od.rpn.clone() } else { zero_rpn() }, size: od.size, const_val: od.const_val.or(Some(0)) },
            }
        }
        other => other.clone(),
    }
}
