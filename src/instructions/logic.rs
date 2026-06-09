use super::{
    data_reg, enc, eval_const, imm_rpn, push_word, size_field, size_to_op_size, InsnError,
};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_orand(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;

    match (&operands[0], &operands[1]) {
        // #imm, Dn → 主命令 (AND/OR) + 即値EA (HASの挙動: C27C形式)
        (EffectiveAddress::Immediate(_), EffectiveAddress::DataReg(dn)) => {
            let imm_enc = enc(&operands[0], op_size)?;
            let word = base | sz | ((*dn as u16) << 9) | (imm_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&imm_enc.ext_bytes);
            Ok(v)
        }
        // #imm, <other ea> → ANDI/ORI
        (EffectiveAddress::Immediate(_), _) => {
            let imm_base = if base & 0x4000 != 0 {
                0x0200u16
            } else {
                0x0000u16
            };
            encode_orandeorimm(imm_base, size, operands)
        }
        // <ea>, Dn: direction = 0（Dn,Dn の場合もこちらが優先）
        (src, EffectiveAddress::DataReg(dn)) => {
            let src_enc = enc(src, op_size)?;
            let word = base | sz | ((*dn as u16) << 9) | (src_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&src_enc.ext_bytes);
            Ok(v)
        }
        // Dn, <mem ea>: direction = 1（宛先がメモリの場合のみ）
        (EffectiveAddress::DataReg(dn), dst) => {
            let dst_enc = enc(dst, op_size)?;
            let word = base | 0x0100 | sz | ((*dn as u16) << 9) | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&dst_enc.ext_bytes);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

pub fn encode_eor(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;

    match (&operands[0], &operands[1]) {
        // #imm, <ea> → EORI
        (EffectiveAddress::Immediate(_), _) => encode_orandeorimm(0x0A00, size, operands),
        // Dn, <ea>
        (EffectiveAddress::DataReg(dn), dst) => {
            let dst_enc = enc(dst, op_size)?;
            let word = base | sz | ((*dn as u16) << 9) | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&dst_enc.ext_bytes);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

pub fn encode_orandeorimm(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;

    match &operands[1] {
        EffectiveAddress::CcrReg => {
            let v = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            let word = base | 0x003C;
            let mut out = Vec::new();
            push_word(&mut out, word);
            push_word(&mut out, v as u16);
            return Ok(out);
        }
        EffectiveAddress::SrReg => {
            let v = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            let word = base | 0x007C;
            let mut out = Vec::new();
            push_word(&mut out, word);
            push_word(&mut out, v as u16);
            return Ok(out);
        }
        _ => {}
    }

    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let imm_enc = enc(&operands[0], op_size)?;
    let dst_enc = enc(&operands[1], op_size)?;
    let word = base | sz | (dst_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&imm_enc.ext_bytes);
    v.extend_from_slice(&dst_enc.ext_bytes);
    Ok(v)
}

pub fn encode_exg(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let (mode, rx, ry) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(x), EffectiveAddress::DataReg(y)) => (0x08u16, *x, *y),
        (EffectiveAddress::AddrReg(x), EffectiveAddress::AddrReg(y)) => (0x09u16, *x, *y),
        (EffectiveAddress::DataReg(x), EffectiveAddress::AddrReg(y)) => (0x11u16, *x, *y),
        (EffectiveAddress::AddrReg(x), EffectiveAddress::DataReg(y)) => (0x11u16, *y, *x),
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = 0xC100u16 | ((rx as u16) << 9) | (mode << 3) | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

pub fn encode_bchclst(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    match (&operands[0], &operands[1]) {
        (EffectiveAddress::Immediate(rpn), dst) => {
            let bit_num = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            if !(0..=31).contains(&bit_num) {
                return Err(InsnError::OutOfRange {
                    value: bit_num,
                    min: 0,
                    max: 31,
                });
            }
            let dst_enc = enc(dst, 0)?;
            let word = base | 0x0800 | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_word(&mut v, bit_num as u16);
            v.extend_from_slice(&dst_enc.ext_bytes);
            Ok(v)
        }
        (EffectiveAddress::DataReg(dn), dst) => {
            let dst_enc = enc(dst, 0)?;
            let word = base | 0x0100 | ((*dn as u16) << 9) | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&dst_enc.ext_bytes);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

pub fn encode_btst(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_bchclst(0x0000, operands)
}

pub fn encode_chk(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0080,
        SizeCode::Long => 0x0000,
        _ => return Err(InsnError::InvalidSize),
    };
    let op_size = size_to_op_size(size)?;
    let src_enc = enc(&operands[0], op_size)?;
    let word = 0x4100u16 | sz_bit | ((dn as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}
