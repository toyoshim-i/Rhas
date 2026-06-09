use super::{addr_reg, data_reg, enc, imm_rpn, push_word, size_field, size_to_op_size, InsnError};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_cmp(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    // #imm, <ea> (non-Dn/An) → CMPI
    if imm_rpn(&operands[0]).is_some()
        && data_reg(&operands[1]).is_none()
        && addr_reg(&operands[1]).is_none()
    {
        return encode_cmpi(0x0C00, size, operands);
    }
    // <ea>, An → CMPA
    if addr_reg(&operands[1]).is_some() {
        return encode_cmpa(0xB0C0, size, operands);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let src_enc = enc(&operands[0], op_size)?;
    let word = base | sz | ((dn as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

pub fn encode_cmpi(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    super::arith::encode_subaddi(base, size, operands)
}

pub fn encode_cmpa(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    super::arith::encode_sbadcpa(base, size, operands)
}

pub fn encode_cmpm(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let (ay, ax) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::AddrRegPostInc(y), EffectiveAddress::AddrRegPostInc(x)) => (*y, *x),
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = base | sz | ((ax as u16) << 9) | (ay as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}
