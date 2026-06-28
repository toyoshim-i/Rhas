use super::ea::{ea_ext_size_for_insn, resolve_ea_with_ext, EaExtKind};
use super::P3Ctx;
use super::{emit_abs_xref, emit_pc_rel_rpn, emit_rofst, emit_rpn_expression};
use crate::addressing::EffectiveAddress;
use crate::error::ErrorCode;
use crate::expr::rpn::RPNToken;
use crate::instructions::encode_insn;
use crate::symbol::types::{InsnHandler, SizeCode};

pub(super) fn process_deferred(
    ctx: &mut P3Ctx<'_>,
    base: u16,
    handler: InsnHandler,
    size: SizeCode,
    ops: &[EffectiveAddress],
) {
    // EA 内の RPN を評価し、外部参照は 0 に置き換えて種別を記録
    let mut resolved_ops: Vec<EffectiveAddress> = Vec::with_capacity(ops.len());
    let mut ext_info: Vec<Option<EaExtKind>> = Vec::with_capacity(ops.len());
    for ea in ops {
        let (resolved, ext_kind) = resolve_ea_with_ext(ctx, ea);
        ext_info.push(ext_kind);
        resolved_ops.push(resolved);
    }

    let has_ext = ext_info.iter().any(|e| e.is_some());

    if matches!(handler, InsnHandler::DBcc) {
        let pc = ctx.location();
        let dn = match resolved_ops.first() {
            Some(EffectiveAddress::DataReg(n)) => *n,
            _ => {
                ctx.emit_zeros(4);
                ctx.error_code(ErrorCode::IlAdr, None);
                return;
            }
        };
        let opcode_word = base | (dn as u16);
        match ext_info.get(1) {
            Some(None) | Some(Some(EaExtKind::SectionAbs(_))) => {
                // 内部参照 (SectionAbs = 解決済み同一セクションラベル):
                // resolve_ea_with_ext で解決済みの AbsLong からターゲットアドレスを取得
                let target_addr = match resolved_ops.get(1) {
                    Some(EffectiveAddress::AbsLong(rpn)) => match rpn.first() {
                        Some(RPNToken::Value(v)) => *v as i32,
                        _ => 0,
                    },
                    _ => 0,
                };
                let disp = target_addr - (pc as i32 + 2);
                if (-32768..=32767).contains(&disp) {
                    let dw = disp as i16 as u16;
                    ctx.emit(&[
                        (opcode_word >> 8) as u8,
                        (opcode_word & 0xFF) as u8,
                        (dw >> 8) as u8,
                        (dw & 0xFF) as u8,
                    ]);
                } else {
                    ctx.emit_zeros(4);
                    ctx.error_code(ErrorCode::IlRelOutside, None);
                }
            }
            Some(Some(EaExtKind::SimpleAbs(name))) => {
                // 外部参照: PC相対リロケーションレコードを生成
                let name = name.clone();
                let xref_num = ctx.get_or_add_xref(name);
                ctx.emit(&[(opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8]);
                let loc = ctx.location(); // displacement word のアドレス
                ctx.flush_code_buf();
                ctx.flush_dsb();
                ctx.advance(2);
                let sect = ctx.cur_sect;
                ctx.code_body.extend_from_slice(&[0x65, sect]);
                ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                ctx.code_body.push((xref_num >> 8) as u8);
                ctx.code_body.push(xref_num as u8);
            }
            _ => {
                ctx.emit_zeros(4);
                ctx.error_code(ErrorCode::IlAdr, None);
            }
        }
        return;
    }

    // FBcc は PC 相対分岐をここで解決する
    if matches!(handler, InsnHandler::FBcc) {
        let pc = ctx.location();
        match ext_info.first() {
            Some(None) | Some(Some(EaExtKind::SectionAbs(_))) => {
                // 内部参照 (SectionAbs = 解決済み同一セクションラベル)
                let target_addr = match resolved_ops.first() {
                    Some(EffectiveAddress::AbsLong(rpn))
                    | Some(EffectiveAddress::AbsShort(rpn)) => match rpn.first() {
                        Some(RPNToken::Value(v)) => *v as i32,
                        _ => {
                            ctx.emit_zeros(4);
                            ctx.error_code(ErrorCode::IlAdr, None);
                            return;
                        }
                    },
                    _ => {
                        ctx.emit_zeros(4);
                        ctx.error_code(ErrorCode::IlAdr, None);
                        return;
                    }
                };
                let disp = target_addr - (pc as i32 + 2);
                let mut opcode_word = base;
                match size {
                    SizeCode::Long => {
                        opcode_word |= 0x0040;
                        ctx.emit(&[
                            (opcode_word >> 8) as u8,
                            (opcode_word & 0xFF) as u8,
                            ((disp >> 24) & 0xFF) as u8,
                            ((disp >> 16) & 0xFF) as u8,
                            ((disp >> 8) & 0xFF) as u8,
                            (disp & 0xFF) as u8,
                        ]);
                    }
                    SizeCode::Word | SizeCode::None => {
                        if !(-32768..=32767).contains(&disp) {
                            if matches!(size, SizeCode::None) {
                                opcode_word |= 0x0040;
                                ctx.emit(&[
                                    (opcode_word >> 8) as u8,
                                    (opcode_word & 0xFF) as u8,
                                    ((disp >> 24) & 0xFF) as u8,
                                    ((disp >> 16) & 0xFF) as u8,
                                    ((disp >> 8) & 0xFF) as u8,
                                    (disp & 0xFF) as u8,
                                ]);
                            } else {
                                ctx.emit_zeros(4);
                                ctx.error_code(ErrorCode::IlRelOutside, None);
                            }
                        } else {
                            let dw = disp as i16 as u16;
                            ctx.emit(&[
                                (opcode_word >> 8) as u8,
                                (opcode_word & 0xFF) as u8,
                                (dw >> 8) as u8,
                                (dw & 0xFF) as u8,
                            ]);
                        }
                    }
                    _ => {
                        ctx.emit_zeros(4);
                        ctx.error_code(ErrorCode::IlSize, None);
                    }
                }
            }
            Some(Some(EaExtKind::SimpleAbs(name))) => {
                // 外部参照: PC相対リロケーションレコードを生成
                match size {
                    SizeCode::Word | SizeCode::None => {
                        let name = name.clone();
                        let xref_num = ctx.get_or_add_xref(name);
                        ctx.emit(&[(base >> 8) as u8, (base & 0xFF) as u8]);
                        let loc = ctx.location(); // ディスプレースメントワードのアドレス
                        ctx.advance(2);
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        let sect = ctx.cur_sect;
                        ctx.code_body.extend_from_slice(&[0x65, sect]);
                        ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                        ctx.code_body.push((xref_num >> 8) as u8);
                        ctx.code_body.push(xref_num as u8);
                    }
                    SizeCode::Long => {
                        // .l 形式 of external ref: RPN Relocation
                        let name = name.clone();
                        let xref_num = ctx.get_or_add_xref(name);
                        let opcode_word = base | 0x0040;
                        ctx.emit(&[(opcode_word >> 8) as u8, (opcode_word & 0xFF) as u8]);
                        let base_addr = ctx.location(); // displacement base = pc + 2
                        ctx.advance(4);
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_pc_rel_rpn(ctx, xref_num, base_addr, 0x92);
                    }
                    _ => {
                        ctx.emit_zeros(4);
                        ctx.error_code(ErrorCode::IlSize, None);
                    }
                }
            }
            _ => {
                ctx.emit_zeros(4);
                ctx.error_code(ErrorCode::IlAdr, None);
            }
        }
        return;
    }

    // FDBcc は opcode + cond + disp16 をここで解決する
    if matches!(handler, InsnHandler::FDBcc) {
        let pc = ctx.location();
        let dn = match resolved_ops.first() {
            Some(EffectiveAddress::DataReg(n)) => *n,
            _ => {
                ctx.emit_zeros(6);
                ctx.error_code(ErrorCode::IlAdr, None);
                return;
            }
        };
        let opcode_word = 0xF048u16 | (base & 0x0E00) | (dn as u16);
        let cond_word = base & 0x001F;
        match ext_info.get(1) {
            Some(None) | Some(Some(EaExtKind::SectionAbs(_))) => {
                // 内部参照 (SectionAbs = 解決済み同一セクションラベル)
                let target_addr = match resolved_ops.get(1) {
                    Some(EffectiveAddress::AbsLong(rpn))
                    | Some(EffectiveAddress::AbsShort(rpn)) => match rpn.first() {
                        Some(RPNToken::Value(v)) => *v as i32,
                        _ => {
                            ctx.emit_zeros(6);
                            ctx.error_code(ErrorCode::IlAdr, None);
                            return;
                        }
                    },
                    _ => {
                        ctx.emit_zeros(6);
                        ctx.error_code(ErrorCode::IlAdr, None);
                        return;
                    }
                };
                let disp = target_addr - (pc as i32 + 4);
                if !(-32768..=32767).contains(&disp) {
                    ctx.emit_zeros(6);
                    ctx.error_code(ErrorCode::IlRelOutside, None);
                    return;
                }
                let dw = disp as i16 as u16;
                ctx.emit(&[
                    (opcode_word >> 8) as u8,
                    (opcode_word & 0xFF) as u8,
                    (cond_word >> 8) as u8,
                    (cond_word & 0xFF) as u8,
                    (dw >> 8) as u8,
                    (dw & 0xFF) as u8,
                ]);
            }
            Some(Some(EaExtKind::SimpleAbs(name))) => {
                // 外部参照: opcode + cond 出力後、PC相対リロケーションレコードを生成
                let name = name.clone();
                let xref_num = ctx.get_or_add_xref(name);
                ctx.emit(&[
                    (opcode_word >> 8) as u8,
                    (opcode_word & 0xFF) as u8,
                    (cond_word >> 8) as u8,
                    (cond_word & 0xFF) as u8,
                ]);
                let loc = ctx.location(); // displacement word のアドレス
                ctx.advance(2);
                ctx.flush_code_buf();
                ctx.flush_dsb();
                let sect = ctx.cur_sect;
                ctx.code_body.extend_from_slice(&[0x65, sect]);
                ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                ctx.code_body.push((xref_num >> 8) as u8);
                ctx.code_body.push(xref_num as u8);
            }
            _ => {
                ctx.emit_zeros(6);
                ctx.error_code(ErrorCode::IlAdr, None);
            }
        }
        return;
    }

    match encode_insn(base, handler, size, &resolved_ops) {
        Ok(bytes) => {
            if !has_ext {
                // 外部参照なし → そのまま出力
                ctx.emit(&bytes);
                return;
            }
            // 外部参照あり → バイト列を分割してリロケーションレコードを挿入
            // bytes = opcode(2) + [extra_insn_bytes] + op0_ext + op1_ext + ...
            // CMP2/CHK2 等は opcode(2) の後に拡張ワードがある
            let total_ext_sz: usize = resolved_ops
                .iter()
                .enumerate()
                .map(|(i, ea)| {
                    if matches!(handler, InsnHandler::SubAddQ) && i == 0 {
                        0
                    } else {
                        ea_ext_size_for_insn(ea, size) as usize
                    }
                })
                .sum();
            let extra_insn_sz = bytes.len() - 2 - total_ext_sz;
            ctx.emit(&bytes[..2 + extra_insn_sz]);
            let mut pos = 2 + extra_insn_sz;
            for (i, ea) in resolved_ops.iter().enumerate() {
                // SubAddQ (ADDQ/SUBQ): immediate count is embedded in opcode bits, not as extension word
                let ext_sz = if matches!(handler, InsnHandler::SubAddQ) && i == 0 {
                    0
                } else {
                    ea_ext_size_for_insn(ea, size) as usize
                };
                if ext_sz == 0 {
                    continue;
                }
                match &ext_info[i] {
                    Some(EaExtKind::SimpleAbs(name)) => {
                        // シンプルな絶対外部参照 → $41/$42 FF xref_num
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_abs_xref(&mut ctx.code_body, ext_sz as u8, xref_num);
                        ctx.advance(ext_sz as u32);
                    }
                    Some(EaExtKind::ExtWithOffset(name, offset)) => {
                        // XREF + 定数オフセット → ROFST レコード
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        let offset = *offset;
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_rofst(&mut ctx.code_body, ext_sz as u8, xref_num, offset);
                        ctx.advance(ext_sz as u32);
                    }
                    Some(EaExtKind::PcRel(name)) => {
                        // PC相対外部参照 → $65 sect loc4 xref_num
                        // オペコードバイトはすでに code_buf に入っている
                        let xref_num = ctx.get_or_add_xref(name.clone());
                        let loc = ctx.location(); // displacement スロットのアドレス
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        ctx.advance(ext_sz as u32); // displacement スロット分進める
                        let sect = ctx.cur_sect;
                        ctx.code_body.extend_from_slice(&[0x65, sect]);
                        ctx.code_body.extend_from_slice(&loc.to_be_bytes());
                        ctx.code_body.push((xref_num >> 8) as u8);
                        ctx.code_body.push(xref_num as u8);
                    }
                    Some(EaExtKind::Complex(rpn)) => {
                        // 複合外部式 → RPN 式レコード
                        let rpn = rpn.clone();
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        emit_rpn_expression(ctx, &rpn, ext_sz as u8);
                    }
                    Some(EaExtKind::SectionAbs(sect)) => {
                        // セクション内絶対参照 → $41/$42 sect value
                        ctx.flush_code_buf();
                        ctx.flush_dsb();
                        let tag = if ext_sz <= 2 { 0x41u8 } else { 0x42u8 };
                        ctx.code_body.push(tag);
                        ctx.code_body.push(*sect);
                        if pos + ext_sz <= bytes.len() {
                            ctx.code_body.extend_from_slice(&bytes[pos..pos + ext_sz]);
                        }
                        ctx.advance(ext_sz as u32);
                    }
                    None => {
                        // 内部参照 → バイトをそのまま出力
                        if pos + ext_sz <= bytes.len() {
                            ctx.emit(&bytes[pos..pos + ext_sz]);
                        }
                    }
                }
                pos += ext_sz;
            }
        }
        Err(e) => {
            // 未解決のまま → ゼロバイトで埋める
            let est = 2 + resolved_ops
                .iter()
                .map(|ea| ea_ext_size_for_insn(ea, size))
                .sum::<u32>();
            ctx.emit_zeros(est);
            let code = match e {
                crate::instructions::InsnError::InvalidSize => ErrorCode::IlSize,
                crate::instructions::InsnError::OutOfRange { min, max, .. } => {
                    if min == -128 && max == 127 {
                        ErrorCode::IlQuickMoveQ
                    } else if min == 1 && max == 8 {
                        if matches!(handler, InsnHandler::SftRot | InsnHandler::Asl) {
                            ErrorCode::IlSft
                        } else {
                            ErrorCode::IlQuickAddSubQ
                        }
                    } else {
                        ErrorCode::Overflow
                    }
                }
                crate::instructions::InsnError::OperandCount => ErrorCode::Expr,
                _ => ErrorCode::IlAdr,
            };
            ctx.error_code(code, None);
        }
    }
}
