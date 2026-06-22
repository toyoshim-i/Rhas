use super::{
    addr_reg, cas2_reg, data_reg, enc, eval_const, imm_rpn, push_long, push_word,
    size_field, size_to_op_size, InsnError,
};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_bcc(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.is_empty() {
        let mut v = Vec::with_capacity(2);
        push_word(&mut v, base);
        return Ok(v);
    }
    // 分岐ターゲットはシンボル参照（PC相対）が必要 → Phase 7 で処理
    Err(InsnError::DeferToLinker)
}

pub fn encode_dbcc(_base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    // ブランチターゲットは PC 相対 → DeferToLinker
    let _ = dn;
    Err(InsnError::DeferToLinker)
}

pub fn encode_scc(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 0)?;
    let word = base | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_link(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let rpn = imm_rpn(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let disp = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    match size {
        SizeCode::Word | SizeCode::None => {
            if disp < i16::MIN as i32 || disp > i16::MAX as i32 {
                return Err(InsnError::OutOfRange {
                    value: disp,
                    min: -32768,
                    max: 32767,
                });
            }
            let word = 0x4E50u16 | (an as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_word(&mut v, disp as u16);
            Ok(v)
        }
        SizeCode::Long => {
            // Long form link.l (68020+): 0x4808
            let word = 0x4808u16 | (an as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_long(&mut v, disp as u32);
            Ok(v)
        }
        _ => Err(InsnError::InvalidSize),
    }
}

pub fn encode_unlk(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let word = 0x4E58u16 | (an as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

pub fn encode_trap(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let vec_num = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if !(0..=15).contains(&vec_num) {
        return Err(InsnError::OutOfRange {
            value: vec_num,
            min: 0,
            max: 15,
        });
    }
    let word = 0x4E40u16 | (vec_num as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

pub fn encode_stoprtd(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    let mut v = Vec::new();
    push_word(&mut v, base);
    push_word(&mut v, val as u16);
    Ok(v)
}

pub fn encode_divsl_ul(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 3 {
        return Err(InsnError::OperandCount);
    }
    let dr = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let dq = data_reg(&operands[2]).ok_or(InsnError::InvalidOperand)?;
    let src_enc = enc(&operands[0], 2)?;
    let sign_bit = if (base & 1) != 0 { 0x0800u16 } else { 0 };
    let real_base = base & !1;
    let ext = ((dq as u16) << 12) | sign_bit | (dr as u16);
    let mut v = Vec::new();
    push_word(&mut v, real_base | (src_enc.ea_field as u16));
    push_word(&mut v, ext);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

pub fn encode_cas2(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 6 {
        return Err(InsnError::OperandCount);
    }
    let dc1 = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let dc2 = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let du1 = data_reg(&operands[2]).ok_or(InsnError::InvalidOperand)?;
    let du2 = data_reg(&operands[3]).ok_or(InsnError::InvalidOperand)?;
    let rn1 = cas2_reg(&operands[4]).ok_or(InsnError::InvalidOperand)?;
    let rn2 = cas2_reg(&operands[5]).ok_or(InsnError::InvalidOperand)?;
    let opcode = match size {
        SizeCode::Word => 0x0CFCu16,
        SizeCode::Long => 0x0EFCu16,
        _ => return Err(InsnError::InvalidSize),
    };
    let ext1 = ((rn1 as u16) << 12) | ((du1 as u16) << 6) | (dc1 as u16);
    let ext2 = ((rn2 as u16) << 12) | ((du2 as u16) << 6) | (dc2 as u16);
    let mut v = Vec::new();
    push_word(&mut v, opcode);
    push_word(&mut v, ext1);
    push_word(&mut v, ext2);
    Ok(v)
}

pub fn encode_extb(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let r = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let mut v = Vec::new();
    push_word(&mut v, 0x49C0 | (r as u16));
    Ok(v)
}

pub fn encode_bkpt(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let n = eval_const(rpn).ok_or(InsnError::DeferToLinker)? & 7;
    let mut v = Vec::new();
    push_word(&mut v, 0x4848 | (n as u16));
    Ok(v)
}

pub fn encode_trapcc(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let mut v = Vec::new();
    if operands.is_empty() {
        push_word(&mut v, base | 0x0004);
    } else {
        let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
        let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
        match size {
            SizeCode::Word | SizeCode::None => {
                push_word(&mut v, base | 0x0002);
                push_word(&mut v, val as u16);
            }
            SizeCode::Long => {
                push_word(&mut v, base | 0x0003);
                push_long(&mut v, val as u32);
            }
            _ => return Err(InsnError::InvalidSize),
        }
    }
    Ok(v)
}

fn parse_bitfield_ext(ops: &[EffectiveAddress]) -> Option<(u16, usize)> {
    if ops.len() < 3 {
        return None;
    }
    let offset_ext = match &ops[1] {
        EffectiveAddress::DataReg(r) => 0x0800u16 | (((*r & 7) as u16) << 6),
        EffectiveAddress::Immediate(rpn) => {
            let v = eval_const(rpn)? as u16 & 0x1F;
            v << 6
        }
        _ => return None,
    };
    let width_ext = match &ops[2] {
        EffectiveAddress::DataReg(r) => 0x0020u16 | ((*r & 7) as u16),
        EffectiveAddress::Immediate(rpn) => eval_const(rpn)? as u16 & 0x1F,
        _ => return None,
    };
    Some((offset_ext | width_ext, 3))
}

pub fn encode_bitfield_1ea(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() < 3 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 1u8)?;
    let (bf_ext, _) = parse_bitfield_ext(operands).ok_or(InsnError::InvalidOperand)?;
    let mut v = Vec::new();
    push_word(&mut v, base | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_bitfield_extract(
    base: u16,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() < 4 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 1u8)?;
    let (mut bf_ext, n) = parse_bitfield_ext(operands).ok_or(InsnError::InvalidOperand)?;
    let dn = data_reg(&operands[n]).ok_or(InsnError::InvalidOperand)?;
    bf_ext |= (dn as u16) << 12;
    let mut v = Vec::new();
    push_word(&mut v, base | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_bfins(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() < 4 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let ea_enc = enc(&operands[1], 1u8)?;
    let (mut bf_ext, _) = parse_bitfield_ext(&operands[1..]).ok_or(InsnError::InvalidOperand)?;
    bf_ext |= (dn as u16) << 12;
    let mut v = Vec::new();
    push_word(&mut v, 0xEFC0 | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_moves(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz_bits = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let (rn_idx, ea_idx, dir_bit) = if is_reg(&operands[0]) {
        (0, 1, 0x0800u16)
    } else {
        (1, 0, 0u16)
    };
    let ea_enc = enc(&operands[ea_idx], op_size)?;
    let rn = reg_code(&operands[rn_idx]).ok_or(InsnError::InvalidOperand)?;
    let ext_word = dir_bit | ((rn as u16) << 12);
    let mut v = Vec::new();
    push_word(&mut v, base | sz_bits | (ea_enc.ea_field as u16));
    push_word(&mut v, ext_word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_movec(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let (reg_op, creg_op, dir) = if is_reg(&operands[0]) {
        (&operands[0], &operands[1], base | 0x0001)
    } else {
        (&operands[1], &operands[0], base)
    };
    let rn = reg_code(reg_op).ok_or(InsnError::InvalidOperand)?;
    let creg = match creg_op {
        EffectiveAddress::Immediate(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 0x0FFF
        }
        EffectiveAddress::AbsLong(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 0x0FFF
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let ext = ((rn as u16) << 12) | creg;
    let mut v = Vec::new();
    push_word(&mut v, dir);
    push_word(&mut v, ext);
    Ok(v)
}

pub fn encode_packunpk(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 3 {
        return Err(InsnError::OperandCount);
    }
    let adj_rpn = imm_rpn(&operands[2]).ok_or(InsnError::InvalidOperand)?;
    let adj = eval_const(adj_rpn).ok_or(InsnError::DeferToLinker)? as u16;
    let (word, mode_bit) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(s), EffectiveAddress::DataReg(d)) => {
            let w = base | ((*d as u16) << 9) | (*s as u16);
            (w, false)
        }
        (EffectiveAddress::AddrRegPreDec(s), EffectiveAddress::AddrRegPreDec(d)) => {
            let w = base | 0x0008 | ((*d as u16) << 9) | (*s as u16);
            (w, true)
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let _ = mode_bit;
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, adj);
    Ok(v)
}

pub fn encode_cas(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let _ = base;
    if operands.len() != 3 {
        return Err(InsnError::OperandCount);
    }
    let dc = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let du = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[2], op_size)?;
    let sz_bits = match size {
        SizeCode::Byte => 0x0200u16,
        SizeCode::Word => 0x0400u16,
        SizeCode::Long => 0x0600u16,
        _ => return Err(InsnError::InvalidSize),
    };
    let word = 0x08C0 | sz_bits | (ea_enc.ea_field as u16);
    let ext = ((du as u16) << 6) | (dc as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_cmpchk2(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[0], op_size)?;
    let rn = reg_code(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let sz_bits = match size {
        SizeCode::Byte => 0x0000u16,
        SizeCode::Word => 0x0200u16,
        SizeCode::Long => 0x0400u16,
        _ => return Err(InsnError::InvalidSize),
    };
    let is_chk2 = base == 0x0800;
    let ext = ((rn as u16) << 12) | if is_chk2 { 0x0800u16 } else { 0u16 };
    let mut v = Vec::new();
    push_word(&mut v, 0x00C0 | sz_bits | (ea_enc.ea_field as u16));
    push_word(&mut v, ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_move16(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    match (&operands[0], &operands[1]) {
        (EffectiveAddress::AddrRegPostInc(ax), EffectiveAddress::AddrRegPostInc(ay)) => {
            let mut v = Vec::new();
            push_word(&mut v, 0xF620 | (*ax as u16));
            push_word(&mut v, 0x8000 | ((*ay as u16) << 12));
            Ok(v)
        }
        (EffectiveAddress::AddrRegPostInc(ax), EffectiveAddress::AbsLong(rpn)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF600 | (*ax as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        (EffectiveAddress::AbsLong(rpn), EffectiveAddress::AddrRegPostInc(ay)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF608 | (*ay as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        (EffectiveAddress::AddrRegInd(ax), EffectiveAddress::AbsLong(rpn)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF610 | (*ax as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        (EffectiveAddress::AbsLong(rpn), EffectiveAddress::AddrRegInd(ay)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF618 | (*ay as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

pub fn encode_cinvpush_lp(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let cache = match &operands[0] {
        EffectiveAddress::Immediate(rpn)
        | EffectiveAddress::AbsLong(rpn)
        | EffectiveAddress::AbsShort(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 3
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let an = match &operands[1] {
        EffectiveAddress::AddrReg(n) | EffectiveAddress::AddrRegInd(n) => *n,
        _ => return Err(InsnError::InvalidOperand),
    };
    let mut v = Vec::new();
    push_word(&mut v, base | (cache << 6) | (an as u16));
    Ok(v)
}

pub fn encode_cinvpush_a(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let cache = match &operands[0] {
        EffectiveAddress::Immediate(rpn)
        | EffectiveAddress::AbsLong(rpn)
        | EffectiveAddress::AbsShort(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 3
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let mut v = Vec::new();
    push_word(&mut v, base | (cache << 6));
    Ok(v)
}

fn reg_code(ea: &EffectiveAddress) -> Option<u8> {
    match ea {
        EffectiveAddress::DataReg(r) => Some(*r),
        EffectiveAddress::AddrReg(r) => Some(8 + *r),
        _ => None,
    }
}

fn is_reg(ea: &EffectiveAddress) -> bool {
    matches!(
        ea,
        EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_)
    )
}
