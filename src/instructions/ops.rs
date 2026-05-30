use super::{
    addr_reg, data_reg, enc, eval_const, imm_rpn, push_word, size_field, size_to_op_size, InsnError,
};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_subadd(
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
        // #imm, An → ADDA/SUBA
        (EffectiveAddress::Immediate(_), EffectiveAddress::AddrReg(_)) => {
            let adda_base = if base & 0x4000 != 0 {
                0xD0C0u16
            } else {
                0x90C0u16
            };
            encode_sbadcpa(adda_base, size, operands)
        }
        // #imm, Dn → 主命令 (ADD/SUB) + 即値EA (HASの挙動: D03C形式)
        (EffectiveAddress::Immediate(_), EffectiveAddress::DataReg(dn)) => {
            let imm_enc = enc(&operands[0], op_size)?;
            let word = base | sz | ((*dn as u16) << 9) | (imm_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&imm_enc.ext_bytes);
            Ok(v)
        }
        // #imm, <other ea> → ADDI/SUBI
        (EffectiveAddress::Immediate(_), dst) => {
            let imm_base = if base & 0x4000 != 0 {
                0x0600u16
            } else {
                0x0400u16
            };
            encode_subaddi(imm_base, size, &[operands[0].clone(), dst.clone()])
        }
        // <ea>, An → ADDA/SUBA
        (_, EffectiveAddress::AddrReg(_)) => {
            let adda_base = if base & 0x4000 != 0 {
                0xD0C0u16
            } else {
                0x90C0u16
            };
            encode_sbadcpa(adda_base, size, operands)
        }
        // <ea>, Dn → dir=0（Dn,Dn の場合もこちらが優先: dest=Dn なら方向=0）
        (src, EffectiveAddress::DataReg(dn)) => {
            let src_enc = enc(src, op_size)?;
            let word = base | sz | ((*dn as u16) << 9) | (src_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&src_enc.ext_bytes);
            Ok(v)
        }
        // Dn, <mem ea> → dir=1（宛先がメモリの場合のみ）
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

pub fn encode_subaddq(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if !(1..=8).contains(&val) {
        return Err(InsnError::OutOfRange {
            value: val,
            min: 1,
            max: 8,
        });
    }
    let qval = if val == 8 { 0u16 } else { val as u16 };
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let dst_enc = enc(&operands[1], op_size)?;
    let word = base | (qval << 9) | sz | (dst_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&dst_enc.ext_bytes);
    Ok(v)
}

pub fn encode_subaddi(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let _ = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
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

pub fn encode_sbadcpa(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let size_bit: u16 = match size {
        SizeCode::Word => 0x0000,
        SizeCode::Long => 0x0100,
        _ => return Err(InsnError::InvalidSize),
    };
    let op_size = size_to_op_size(size)?;
    let src_enc = enc(&operands[0], op_size)?;
    let word = base | size_bit | ((an as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

pub fn encode_subaddx(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let (mode_bit, ry, rx) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(y), EffectiveAddress::DataReg(x)) => (0x0000u16, *y, *x),
        (EffectiveAddress::AddrRegPreDec(y), EffectiveAddress::AddrRegPreDec(x)) => {
            (0x0008u16, *y, *x)
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = base | sz | ((rx as u16) << 9) | mode_bit | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

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
        return encode_sbadcpa(0xB0C0, size, operands);
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
    encode_subaddi(base, size, operands)
}

pub fn encode_cmpa(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    encode_sbadcpa(base, size, operands)
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

pub fn encode_negnot(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[0], op_size)?;
    let word = base | sz | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_clr(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    encode_negnot(base, size, operands)
}

pub fn encode_tst(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    encode_negnot(base, size, operands)
}

pub fn encode_ext(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0000, // 0x4880
        SizeCode::Long => 0x0040, // 0x48C0
        _ => return Err(InsnError::InvalidSize),
    };
    let word = 0x4880u16 | sz_bit | (dn as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

pub fn encode_swap(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let word = 0x4840u16 | (dn as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
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

pub fn encode_divmul(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if size == SizeCode::Long {
        // 68020+ long form
        let is_signed = (base & 0x0100) != 0;
        let is_div = (base & 0x4000) == 0;
        let long_base = if is_div { 0x4C40u16 } else { 0x4C00u16 };
        let sign_bit = if is_signed { 0x0800u16 } else { 0 };

        if operands.len() == 2 {
            let dq = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
            let src_enc = enc(&operands[0], 2)?;
            let ext = ((dq as u16) << 12) | sign_bit | (dq as u16);
            let mut v = Vec::new();
            push_word(&mut v, long_base | (src_enc.ea_field as u16));
            push_word(&mut v, ext);
            v.extend_from_slice(&src_enc.ext_bytes);
            return Ok(v);
        } else if operands.len() == 3 {
            let dh_or_dr = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
            let dl_or_dq = data_reg(&operands[2]).ok_or(InsnError::InvalidOperand)?;
            let src_enc = enc(&operands[0], 2)?;
            let size_bit = if is_div { 0u16 } else { 0x0400u16 };
            let ext = ((dl_or_dq as u16) << 12) | sign_bit | size_bit | (dh_or_dr as u16);
            let mut v = Vec::new();
            push_word(&mut v, long_base | (src_enc.ea_field as u16));
            push_word(&mut v, ext);
            v.extend_from_slice(&src_enc.ext_bytes);
            return Ok(v);
        }
        return Err(InsnError::OperandCount);
    }

    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let src_enc = enc(&operands[0], 1)?;
    let word = base | ((dn as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
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

pub fn encode_sabcd(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let (mode_bit, ry, rx) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(y), EffectiveAddress::DataReg(x)) => (0x0000u16, *y, *x),
        (EffectiveAddress::AddrRegPreDec(y), EffectiveAddress::AddrRegPreDec(x)) => {
            (0x0008u16, *y, *x)
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = base | ((rx as u16) << 9) | mode_bit | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

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

pub fn encode_decinc(
    base: u16,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[0], op_size)?;
    let word = base | (1u16 << 9) | sz | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}
