use super::{addr_reg, data_reg, enc, eval_const, imm_rpn, push_word, size_to_op_size, InsnError};
use crate::addressing::EffectiveAddress;
use crate::symbol::types::SizeCode;

pub fn encode_jmpjsr(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 1)?;
    let word = base | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

pub fn encode_move(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }

    // 特殊ケース: MOVE <ea>, CCR / MOVE <ea>, SR / MOVE SR, <ea> / MOVE CCR, <ea>
    match (&operands[0], &operands[1]) {
        // MOVE <ea>, CCR  (0x44C0 | src_ea)
        (src, EffectiveAddress::CcrReg) => {
            let src_enc = enc(src, 1)?;
            let word = 0x44C0u16 | (src_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&src_enc.ext_bytes);
            return Ok(v);
        }
        // MOVE <ea>, SR  (0x46C0 | src_ea)
        (src, EffectiveAddress::SrReg) => {
            let src_enc = enc(src, 1)?;
            let word = 0x46C0u16 | (src_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&src_enc.ext_bytes);
            return Ok(v);
        }
        // MOVE SR, <ea>  (0x40C0 | dst_ea)
        (EffectiveAddress::SrReg, dst) => {
            let dst_enc = enc(dst, 1)?;
            let word = 0x40C0u16 | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&dst_enc.ext_bytes);
            return Ok(v);
        }
        // MOVE CCR, <ea>  (0x42C0 | dst_ea) - 68010+
        (EffectiveAddress::CcrReg, dst) => {
            let dst_enc = enc(dst, 1)?;
            let word = 0x42C0u16 | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            v.extend_from_slice(&dst_enc.ext_bytes);
            return Ok(v);
        }
        _ => {}
    }

    let size_top = match size {
        SizeCode::Byte => 0x1000u16,
        SizeCode::Word => 0x3000u16,
        SizeCode::Long => 0x2000u16,
        _ => return Err(InsnError::InvalidSize),
    };
    let op_size = size_to_op_size(size)?;
    let src_enc = enc(&operands[0], op_size)?;
    let dst_enc = enc(&operands[1], op_size)?;

    // 宛先 EA フィールドは mode と reg が入れ替わる
    let dst_mode = ((dst_enc.ea_field >> 3) & 7) as u16;
    let dst_reg = (dst_enc.ea_field & 7) as u16;
    let dst_field = (dst_reg << 9) | (dst_mode << 6);

    let word = size_top | dst_field | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    v.extend_from_slice(&dst_enc.ext_bytes);
    Ok(v)
}

pub fn encode_movea(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let size_bit: u16 = match size {
        SizeCode::Word => 0x1000, // 0x3040 pattern
        SizeCode::Long => 0x0000, // 0x2040 pattern
        _ => return Err(InsnError::InvalidSize),
    };
    let op_size = size_to_op_size(size)?;
    let src_enc = enc(&operands[0], op_size)?;
    // 0x2040: bits 13-12=10(long), bits 8-6=001(An direct), An=0
    let word = 0x2000u16 | size_bit | ((an as u16) << 9) | 0x0040 | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

pub fn encode_moveq(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if !(-128..=127).contains(&val) {
        return Err(InsnError::OutOfRange {
            value: val,
            min: -128,
            max: 127,
        });
    }
    let word = 0x7000u16 | ((dn as u16) << 9) | (val as u8 as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

pub fn encode_movem(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0000,
        SizeCode::Long => 0x0040,
        _ => return Err(InsnError::InvalidSize),
    };
    // operands[0] が Immediate/DataReg/AddrReg → reg→mem 方向
    // operands[1] が Immediate/DataReg/AddrReg → mem→reg 方向
    let (dir_bit, mask, ea) = if let Some(rpn) = imm_rpn(&operands[0]) {
        (
            0x0000u16,
            eval_const(rpn).ok_or(InsnError::DeferToLinker)?,
            &operands[1],
        )
    } else if let Some(rpn) = imm_rpn(&operands[1]) {
        (
            0x0400u16,
            eval_const(rpn).ok_or(InsnError::DeferToLinker)?,
            &operands[0],
        )
    } else if let Some(n) = data_reg(&operands[0]) {
        (0x0000u16, 1i32 << n, &operands[1])
    } else if let Some(n) = addr_reg(&operands[0]) {
        (0x0000u16, 1i32 << (n + 8), &operands[1])
    } else if let Some(n) = data_reg(&operands[1]) {
        (0x0400u16, 1i32 << n, &operands[0])
    } else if let Some(n) = addr_reg(&operands[1]) {
        (0x0400u16, 1i32 << (n + 8), &operands[0])
    } else {
        return Err(InsnError::InvalidOperand);
    };
    // -(An) の場合、レジスタマスクを反転する（D7→bit0, A7→bit8）
    let (mask_word, is_predec) = if matches!(ea, EffectiveAddress::AddrRegPreDec(_)) {
        (reverse_bits16(mask as u16), true)
    } else {
        (mask as u16, false)
    };
    let _ = is_predec; // 使用済みとしてマーク
    let ea_enc = enc(ea, 1)?;
    let word = 0x4880u16 | dir_bit | sz_bit | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, mask_word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

fn reverse_bits16(x: u16) -> u16 {
    x.reverse_bits()
}

pub fn encode_movep(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0000,
        SizeCode::Long => 0x0040,
        _ => return Err(InsnError::InvalidSize),
    };
    // Dn, (d,An) → reg→mem (bit7=1)
    // (d,An), Dn → mem→reg (bit7=0)
    let (dir_bit, dn, an, disp) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(d), EffectiveAddress::AddrRegDisp { an, disp }) => {
            (0x0080u16, *d, *an, disp)
        }
        (EffectiveAddress::AddrRegDisp { an, disp }, EffectiveAddress::DataReg(d)) => {
            (0x0000u16, *d, *an, disp)
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let d_val = if let Some(v) = disp.const_val {
        v
    } else {
        eval_const(&disp.rpn).ok_or(InsnError::DeferToLinker)?
    };
    if d_val < i16::MIN as i32 || d_val > i16::MAX as i32 {
        return Err(InsnError::OutOfRange {
            value: d_val,
            min: -32768,
            max: 32767,
        });
    }
    let word = 0x0108u16 | sz_bit | dir_bit | ((dn as u16) << 9) | (an as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, d_val as u16);
    Ok(v)
}

pub fn encode_lea(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let src_enc = enc(&operands[0], 1)?;
    let word = 0x41C0u16 | ((an as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

pub fn encode_peajsrjmp(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 1)?;
    let word = base | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}
