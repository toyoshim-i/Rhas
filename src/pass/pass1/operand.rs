use super::skip_spaces;
use crate::addressing::{parse_ea, EffectiveAddress, EaError};
use crate::expr::rpn::RPNToken;
use crate::symbol::types::reg;
use crate::symbol::{Symbol, SymbolTable};

pub(super) fn parse_operands(
    line: &[u8],
    mut pos: usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<Vec<EffectiveAddress>, EaError> {
    fn fp_mask_bit(fp_idx: u8) -> u16 {
        1u16 << (7 - (fp_idx & 7))
    }

    fn parse_fp_reg_list_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        // 先頭 FPn を読む
        let start = *pos;
        let mut end = start;
        if end >= line.len() || !line[end].is_ascii_alphabetic() {
            return None;
        }
        while end < line.len() && (line[end].is_ascii_alphanumeric() || line[end] == b'_') {
            end += 1;
        }
        let regno = match sym.lookup_reg(&line[start..end], cpu_type) {
            Some(Symbol::Register { regno, .. }) => *regno,
            _ => return None,
        };
        if !(reg::FP0..=reg::FP7).contains(&regno) {
            return None;
        }

        let mut last_fp = regno - reg::FP0;
        let mut mask = fp_mask_bit(last_fp);
        *pos = end;

        // 区切り（/ または -）がなければリストではない
        let mut p = *pos;
        skip_spaces(line, &mut p);
        if p >= line.len() || (line[p] != b'/' && line[p] != b'-') {
            *pos = saved;
            return None;
        }

        loop {
            skip_spaces(line, pos);
            if *pos >= line.len() {
                break;
            }
            let sep = line[*pos];
            if sep != b'/' && sep != b'-' {
                break;
            }
            *pos += 1;
            skip_spaces(line, pos);

            let rs = *pos;
            let mut re = rs;
            if re >= line.len() || !line[re].is_ascii_alphabetic() {
                *pos = saved;
                return None;
            }
            while re < line.len() && (line[re].is_ascii_alphanumeric() || line[re] == b'_') {
                re += 1;
            }
            let regno2 = match sym.lookup_reg(&line[rs..re], cpu_type) {
                Some(Symbol::Register { regno, .. }) => *regno,
                _ => {
                    *pos = saved;
                    return None;
                }
            };
            if !(reg::FP0..=reg::FP7).contains(&regno2) {
                *pos = saved;
                return None;
            }
            *pos = re;
            let fp2 = regno2 - reg::FP0;

            if sep == b'-' {
                // 範囲指定: 直前の要素と今回要素の範囲を埋める
                let from = last_fp;
                let lo = from.min(fp2);
                let hi = from.max(fp2);
                for i in lo..=hi {
                    mask |= fp_mask_bit(i);
                }
            } else {
                mask |= fp_mask_bit(fp2);
            }
            last_fp = fp2;
        }

        Some(EffectiveAddress::Immediate(vec![
            RPNToken::ValueWord(mask),
            RPNToken::End,
        ]))
    }

    fn parse_fp_ctrl_list_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        let parse_one = |s: &[u8]| -> Option<u16> {
            match sym.lookup_reg(s, cpu_type) {
                Some(Symbol::Register { regno, .. }) => match *regno {
                    reg::FPCR => Some(0x1000),
                    reg::FPSR => Some(0x0800),
                    reg::FPIAR => Some(0x0400),
                    _ => None,
                },
                _ => None,
            }
        };

        let mut p = *pos;
        let mut mask: u16 = 0;
        let mut any = false;
        loop {
            if p >= line.len() || !line[p].is_ascii_alphabetic() {
                break;
            }
            let start = p;
            while p < line.len() && (line[p].is_ascii_alphanumeric() || line[p] == b'_') {
                p += 1;
            }
            if let Some(m) = parse_one(&line[start..p]) {
                mask |= m;
                any = true;
            } else {
                break;
            }
            let mut q = p;
            skip_spaces(line, &mut q);
            if q < line.len() && line[q] == b'/' {
                p = q + 1;
                skip_spaces(line, &mut p);
                continue;
            }
            p = q;
            break;
        }

        if !any {
            *pos = saved;
            return None;
        }
        *pos = p;
        Some(EffectiveAddress::Immediate(vec![
            RPNToken::ValueWord(mask),
            RPNToken::End,
        ]))
    }

    fn parse_fp_pair_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let saved = *pos;

        let parse_fp = |s: &[u8]| -> Option<u8> {
            match sym.lookup_reg(s, cpu_type) {
                Some(Symbol::Register { regno, .. }) if (reg::FP0..=reg::FP7).contains(regno) => {
                    Some(*regno - reg::FP0)
                }
                _ => None,
            }
        };

        // FPc
        let start_c = *pos;
        let mut end_c = start_c;
        if end_c >= line.len() || !line[end_c].is_ascii_alphabetic() {
            return None;
        }
        while end_c < line.len() && (line[end_c].is_ascii_alphanumeric() || line[end_c] == b'_') {
            end_c += 1;
        }
        let fp_c = parse_fp(&line[start_c..end_c])?;

        // :
        let mut p = end_c;
        skip_spaces(line, &mut p);
        if p >= line.len() || line[p] != b':' {
            *pos = saved;
            return None;
        }
        p += 1;
        skip_spaces(line, &mut p);

        // FPs
        let start_s = p;
        let mut end_s = start_s;
        if end_s >= line.len() || !line[end_s].is_ascii_alphabetic() {
            *pos = saved;
            return None;
        }
        while end_s < line.len() && (line[end_s].is_ascii_alphanumeric() || line[end_s] == b'_') {
            end_s += 1;
        }
        let fp_s = match parse_fp(&line[start_s..end_s]) {
            Some(v) => v,
            None => {
                *pos = saved;
                return None;
            }
        };

        *pos = end_s;
        // 0x8000 を FPc:FPs の識別マーカーとして使用。
        let encoded = 0x8000u16 | ((fp_c as u16) << 4) | (fp_s as u16);
        Some(EffectiveAddress::Immediate(vec![
            RPNToken::ValueWord(encoded),
            RPNToken::End,
        ]))
    }

    fn parse_fp_register_token(
        line: &[u8],
        pos: &mut usize,
        sym: &SymbolTable,
        cpu_type: u16,
    ) -> Option<EffectiveAddress> {
        let start = *pos;
        if start >= line.len() {
            return None;
        }
        let c = line[start];
        if !c.is_ascii_alphabetic() && c != b'_' {
            return None;
        }
        let mut end = start + 1;
        while end < line.len() {
            let b = line[end];
            if b.is_ascii_alphanumeric() || b == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        let name = &line[start..end];
        let regno = match sym.lookup_reg(name, cpu_type) {
            Some(Symbol::Register { regno, .. }) => *regno,
            _ => return None,
        };
        let ea = match regno {
            reg::FP0..=reg::FP7 => EffectiveAddress::FpReg(regno - reg::FP0),
            reg::FPCR => EffectiveAddress::FpCtrlReg(0),
            reg::FPSR => EffectiveAddress::FpCtrlReg(1),
            reg::FPIAR => EffectiveAddress::FpCtrlReg(2),
            _ => return None,
        };
        *pos = end;
        Some(ea)
    }

    let mut ops = Vec::new();
    skip_spaces(line, &mut pos);

    loop {
        if pos >= line.len() || line[pos] == b';' {
            break;
        }
        match parse_fp_ctrl_list_token(line, &mut pos, sym, cpu_type)
            .map(Ok)
            .unwrap_or_else(|| {
                parse_fp_reg_list_token(line, &mut pos, sym, cpu_type)
                    .map(Ok)
                    .unwrap_or_else(|| {
                        parse_fp_pair_token(line, &mut pos, sym, cpu_type)
                            .map(Ok)
                            .unwrap_or_else(|| {
                                parse_fp_register_token(line, &mut pos, sym, cpu_type)
                                    .map(Ok)
                                    .unwrap_or_else(|| parse_ea(line, &mut pos, sym, cpu_type))
                            })
                    })
            }) {
            Ok(ea) => {
                ops.push(ea);
                // Bitfield suffix {offset:width}
                if pos < line.len() && line[pos] == b'{' {
                    pos += 1;
                    skip_spaces(line, &mut pos);
                    match parse_ea(line, &mut pos, sym, cpu_type) {
                        Ok(off) => ops.push(abs_to_imm(off)),
                        Err(e) => return Err(e),
                    }
                    skip_spaces(line, &mut pos);
                    if pos < line.len() && line[pos] == b':' {
                        pos += 1;
                        skip_spaces(line, &mut pos);
                        match parse_ea(line, &mut pos, sym, cpu_type) {
                            Ok(w) => ops.push(abs_to_imm(w)),
                            Err(e) => return Err(e),
                        }
                    }
                    skip_spaces(line, &mut pos);
                    if pos < line.len() && line[pos] == b'}' {
                        pos += 1;
                    }
                }
                // Register pair Dn:Dm or EA pair (An):(Am) for CAS2/MULS.L etc.
                if pos < line.len() && line[pos] == b':' {
                    let save = pos;
                    pos += 1;
                    skip_spaces(line, &mut pos);
                    match parse_ea(line, &mut pos, sym, cpu_type) {
                        Ok(pair) => ops.push(pair),
                        Err(_) => {
                            pos = save;
                        }
                    }
                }
            }
            Err(e) => return Err(e),
        }
        skip_spaces(line, &mut pos);
        if pos < line.len() && line[pos] == b',' {
            pos += 1;
            skip_spaces(line, &mut pos);
        } else {
            break;
        }
    }
    Ok(ops)
}

fn abs_to_imm(ea: EffectiveAddress) -> EffectiveAddress {
    match ea {
        EffectiveAddress::AbsShort(rpn) => EffectiveAddress::Immediate(rpn),
        EffectiveAddress::AbsLong(rpn) => EffectiveAddress::Immediate(rpn),
        other => other,
    }
}
