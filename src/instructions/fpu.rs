use super::{eval_const, eval_immediate_u8, fpu_size_code, map_enc_err, push_word, InsnError};
use crate::addressing::{encode::encode_ea, EffectiveAddress};
use crate::symbol::types::SizeCode;

pub fn encode_fnop(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if !operands.is_empty() {
        return Err(InsnError::OperandCount);
    }
    let mut out = Vec::new();
    push_word(&mut out, base);
    push_word(&mut out, 0x0000);
    Ok(out)
}

pub fn encode_fsave_frestore(
    base: u16,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let ea = encode_ea(&operands[0], 2).map_err(map_enc_err)?;
    let mut out = Vec::new();
    push_word(&mut out, base | (ea.ea_field as u16));
    out.extend_from_slice(&ea.ext_bytes);
    Ok(out)
}

pub fn encode_fmovecr(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if !matches!(size, SizeCode::None | SizeCode::Extend) {
        return Err(InsnError::InvalidSize);
    }
    let k = match &operands[0] {
        EffectiveAddress::Immediate(rpn) => eval_immediate_u8(rpn)?,
        _ => return Err(InsnError::InvalidOperand),
    };
    let dst = match operands[1] {
        EffectiveAddress::FpReg(n) => n as u16,
        _ => return Err(InsnError::InvalidOperand),
    };
    if k > 0x7F {
        return Err(InsnError::OutOfRange {
            value: k as i32,
            min: 0,
            max: 0x7F,
        });
    }
    let mut out = Vec::new();
    push_word(&mut out, 0xF000 | (base & 0x0E00));
    push_word(&mut out, 0x5C00 | (dst << 7) | k as u16);
    Ok(out)
}

pub fn encode_fop2(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dst = match operands[1] {
        EffectiveAddress::FpReg(n) => n as u16,
        _ => return Err(InsnError::InvalidOperand),
    };
    let op = base & 0x00FF;
    let cpid = base & 0x0E00;
    let mut w2 = dst << 7 | op;
    let mut out = Vec::new();
    match &operands[0] {
        EffectiveAddress::FpReg(src) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            w2 |= (*src as u16) << 10;
            push_word(&mut out, 0xF000 | cpid);
            push_word(&mut out, w2);
        }
        ea => {
            let sc = if matches!(size, SizeCode::None) {
                2
            } else {
                fpu_size_code(size)?
            };
            w2 |= sc << 10;
            w2 |= 0x4000;
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, w2);
            out.extend_from_slice(&enc.ext_bytes);
        }
    }
    Ok(out)
}

pub fn encode_ftst(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let op = base & 0x00FF;
    let cpid = base & 0x0E00;
    let mut out = Vec::new();
    match &operands[0] {
        EffectiveAddress::FpReg(src) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            let w2 = ((*src as u16) << 10) | op;
            push_word(&mut out, 0xF000 | cpid);
            push_word(&mut out, w2);
        }
        ea => {
            let sc = if matches!(size, SizeCode::None) {
                2
            } else {
                fpu_size_code(size)?
            };
            let w2 = 0x4000 | (sc << 10) | op;
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, w2);
            out.extend_from_slice(&enc.ext_bytes);
        }
    }
    Ok(out)
}

pub fn encode_fmove(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let cpid = base & 0x0E00;
    let mut out = Vec::new();
    match (&operands[0], &operands[1]) {
        (EffectiveAddress::FpReg(src), EffectiveAddress::FpReg(dst)) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            push_word(&mut out, 0xF000 | cpid);
            push_word(&mut out, ((*src as u16) << 10) | ((*dst as u16) << 7));
        }
        (ea, EffectiveAddress::FpReg(dst)) => {
            let sc = if matches!(size, SizeCode::None) {
                2
            } else {
                fpu_size_code(size)?
            };
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0x4000 | (sc << 10) | ((*dst as u16) << 7));
            out.extend_from_slice(&enc.ext_bytes);
        }
        (EffectiveAddress::FpReg(src), ea) => {
            let sc = if matches!(size, SizeCode::None) {
                2
            } else {
                fpu_size_code(size)?
            };
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0x6000 | (sc << 10) | ((*src as u16) << 7));
            out.extend_from_slice(&enc.ext_bytes);
        }
        _ => return Err(InsnError::InvalidOperand),
    }
    Ok(out)
}

pub fn encode_fmovem(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if !matches!(size, SizeCode::None | SizeCode::Long | SizeCode::Extend) {
        return Err(InsnError::InvalidSize);
    }
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let cpid = base & 0x0E00;
    let mut out = Vec::new();
    match (&operands[0], &operands[1]) {
        // fmovem fpcr/fpsr/fpiar,<ea>
        (EffectiveAddress::FpCtrlReg(reg), ea) => {
            if !matches!(size, SizeCode::None | SizeCode::Long) {
                return Err(InsnError::InvalidSize);
            }
            let mask = match reg {
                0 => 0x1000u16, // FPCR
                1 => 0x0800u16, // FPSR
                2 => 0x0400u16, // FPIAR
                _ => return Err(InsnError::InvalidOperand),
            };
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0xA000 | mask);
            out.extend_from_slice(&enc.ext_bytes);
        }
        // fmovem <ea>,fpcr/fpsr/fpiar
        (ea, EffectiveAddress::FpCtrlReg(reg)) => {
            if !matches!(size, SizeCode::None | SizeCode::Long) {
                return Err(InsnError::InvalidSize);
            }
            let mask = match reg {
                0 => 0x1000u16, // FPCR
                1 => 0x0800u16, // FPSR
                2 => 0x0400u16, // FPIAR
                _ => return Err(InsnError::InvalidOperand),
            };
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0x8000 | mask);
            out.extend_from_slice(&enc.ext_bytes);
        }
        // fmovem <fplist>,<ea> (FPn -> mem, static list)
        (EffectiveAddress::Immediate(rpn), ea) => {
            let raw = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16;
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            let ctrl_mask = raw & 0x1C00;
            let ext = if ctrl_mask != 0 {
                if !matches!(size, SizeCode::None | SizeCode::Long) {
                    return Err(InsnError::InvalidSize);
                }
                0xA000u16 | ctrl_mask
            } else {
                if !matches!(size, SizeCode::None | SizeCode::Extend) {
                    return Err(InsnError::InvalidSize);
                }
                let mask = raw & 0x00FF;
                if matches!(ea, EffectiveAddress::AddrRegPreDec(_)) {
                    0xE000u16 | ((mask as u8).reverse_bits() as u16)
                } else {
                    0xF000u16 | mask
                }
            };
            push_word(&mut out, ext);
            out.extend_from_slice(&enc.ext_bytes);
        }
        // fmovem <ea>,<fplist> (mem -> FPn, static list)
        (ea, EffectiveAddress::Immediate(rpn)) => {
            let raw = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16;
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            let ctrl_mask = raw & 0x1C00;
            let ext = if ctrl_mask != 0 {
                if !matches!(size, SizeCode::None | SizeCode::Long) {
                    return Err(InsnError::InvalidSize);
                }
                0x8000u16 | ctrl_mask
            } else {
                if !matches!(size, SizeCode::None | SizeCode::Extend) {
                    return Err(InsnError::InvalidSize);
                }
                0xD000u16 | (raw & 0x00FF)
            };
            push_word(&mut out, ext);
            out.extend_from_slice(&enc.ext_bytes);
        }
        // fmovem <dn>,<ea> (FPn -> mem, dynamic list)
        (EffectiveAddress::DataReg(dn), ea) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            let base = if matches!(ea, EffectiveAddress::AddrRegPreDec(_)) {
                0xE800u16
            } else {
                0xF800u16
            };
            push_word(&mut out, base | ((*dn as u16) << 4));
            out.extend_from_slice(&enc.ext_bytes);
        }
        // fmovem <ea>,<dn> (mem -> FPn, dynamic list)
        (ea, EffectiveAddress::DataReg(dn)) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0xD800u16 | ((*dn as u16) << 4));
            out.extend_from_slice(&enc.ext_bytes);
        }
        _ => return Err(InsnError::InvalidOperand),
    }
    Ok(out)
}

pub fn encode_fsincos(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    let size = if matches!(size, SizeCode::Word) {
        SizeCode::None
    } else {
        size
    };
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let pair = match &operands[1] {
        // pass1 で FPc:FPs を 0x8000 | (FPc<<4) | FPs の Immediate に変換している。
        EffectiveAddress::Immediate(rpn) => {
            let raw = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16;
            if (raw & 0x8000) == 0 {
                return Err(InsnError::InvalidOperand);
            }
            let fp_c = ((raw >> 4) & 0x7) as u16;
            let fp_s = (raw & 0x7) as u16;
            ((fp_s >> 1) << 8) | (0x0030u16 | fp_c | ((fp_s & 1) << 7))
        }
        _ => return Err(InsnError::InvalidOperand),
    };

    let cpid = base & 0x0E00;
    let mut out = Vec::new();
    match &operands[0] {
        EffectiveAddress::FpReg(src) => {
            if !matches!(size, SizeCode::None | SizeCode::Extend) {
                return Err(InsnError::InvalidSize);
            }
            let w2 = ((*src as u16) << 10) | pair;
            push_word(&mut out, 0xF000 | cpid);
            push_word(&mut out, w2);
        }
        ea => {
            let sc = if matches!(size, SizeCode::None) {
                2
            } else {
                fpu_size_code(size)?
            };
            let enc = encode_ea(ea, 2).map_err(map_enc_err)?;
            push_word(&mut out, 0xF000 | cpid | enc.ea_field as u16);
            push_word(&mut out, 0x4000 | (sc << 10) | pair);
            out.extend_from_slice(&enc.ext_bytes);
        }
    }
    Ok(out)
}
