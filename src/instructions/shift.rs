use super::{data_reg, enc, eval_const, push_word, size_field, size_to_op_size, InsnError};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_sftrot(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let sz = size_field(size)?;
    if operands.len() == 1 {
        if size != SizeCode::Word {
            return Err(InsnError::InvalidSize);
        }
        let ea_enc = enc(&operands[0], 1)?;
        let type_bits = (base & 0x0018) >> 3;
        let dir_bit = (base & 0x0100) >> 8;
        let word =
            0xE000u16 | (type_bits << 9) | (dir_bit << 8) | 0x00C0 | (ea_enc.ea_field as u16);
        let mut v = Vec::new();
        push_word(&mut v, word);
        v.extend_from_slice(&ea_enc.ext_bytes);
        return Ok(v);
    }
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dest_dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    match &operands[0] {
        EffectiveAddress::Immediate(rpn) => {
            let count = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            if !(1..=8).contains(&count) {
                return Err(InsnError::OutOfRange {
                    value: count,
                    min: 1,
                    max: 8,
                });
            }
            let cnt = if count == 8 { 0u16 } else { count as u16 };
            let word = (base & 0xFFF8) | sz | (cnt << 9) | (dest_dn as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            Ok(v)
        }
        EffectiveAddress::DataReg(src_dn) => {
            let word = (base & 0xFFF8) | sz | 0x0020 | ((*src_dn as u16) << 9) | (dest_dn as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

pub fn encode_asl(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    encode_sftrot(base, size, operands)
}
