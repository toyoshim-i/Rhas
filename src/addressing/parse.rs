use super::{
    DispSize, Displacement, EaError, EffectiveAddress, IdxSize, IndexSpec, Scale,
};
use crate::expr::{parse_expr, RPNToken};
use crate::symbol::types::reg;
use crate::symbol::{Symbol, SymbolTable};

fn skip_spaces(src: &[u8], pos: &mut usize) {
    while *pos < src.len() && matches!(src[*pos], b' ' | b'\t') {
        *pos += 1;
    }
}

/// レジスタ名の開始文字
fn is_reg_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

/// レジスタ名の継続文字（. は含まない）
fn is_reg_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 現在位置のレジスタ名を読む（成功時のみ pos を進める）
fn try_parse_register(src: &[u8], pos: &mut usize, sym: &SymbolTable, cpu_type: u16) -> Option<u8> {
    let start = *pos;
    let mut end = start;
    if end >= src.len() || !is_reg_start(src[end]) {
        return None;
    }
    while end < src.len() && is_reg_cont(src[end]) {
        end += 1;
    }
    let name = &src[start..end];
    match sym.lookup_reg(name, cpu_type) {
        Some(Symbol::Register { regno, .. }) => {
            *pos = end;
            Some(*regno)
        }
        _ => None,
    }
}

/// サイズ指定 .w / .l / .s を試して読む（成功時のみ pos を進める）
fn try_parse_disp_size(src: &[u8], pos: &mut usize) -> Option<DispSize> {
    if *pos >= src.len() || src[*pos] != b'.' {
        return None;
    }
    let saved = *pos;
    *pos += 1;
    if *pos >= src.len() {
        *pos = saved;
        return None;
    }
    let ch = src[*pos].to_ascii_lowercase();
    // サイズ文字の直後が識別子継続文字でないことを確認（.long などと区別）
    let after = *pos + 1;
    if after < src.len() && src[after].is_ascii_alphanumeric() {
        *pos = saved;
        return None;
    }
    match ch {
        b's' => {
            *pos += 1;
            Some(DispSize::Short)
        }
        b'w' => {
            *pos += 1;
            Some(DispSize::Word)
        }
        b'l' => {
            *pos += 1;
            Some(DispSize::Long)
        }
        _ => {
            *pos = saved;
            None
        }
    }
}

/// インデックスレジスタのサイズ指定 .w / .l を試して読む
fn try_parse_idx_size(src: &[u8], pos: &mut usize) -> IdxSize {
    if *pos >= src.len() || src[*pos] != b'.' {
        return IdxSize::Word;
    }
    let saved = *pos;
    *pos += 1;
    if *pos >= src.len() {
        *pos = saved;
        return IdxSize::Word;
    }
    let ch = src[*pos].to_ascii_lowercase();
    let after = *pos + 1;
    if after < src.len() && src[after].is_ascii_alphanumeric() {
        *pos = saved;
        return IdxSize::Word;
    }
    match ch {
        b'w' => {
            *pos += 1;
            IdxSize::Word
        }
        b'l' => {
            *pos += 1;
            IdxSize::Long
        }
        _ => {
            *pos = saved;
            IdxSize::Word
        }
    }
}

/// スケールファクタ *1/*2/*4/*8 を試して読む（なければ S1）
fn try_parse_scale(src: &[u8], pos: &mut usize) -> Result<Scale, EaError> {
    if *pos >= src.len() || src[*pos] != b'*' {
        return Ok(Scale::S1);
    }
    let saved = *pos;
    *pos += 1;
    let start = *pos;
    while *pos < src.len() && src[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == start {
        *pos = saved;
        return Ok(Scale::S1);
    }
    let val: u32 = src[start..*pos]
        .iter()
        .fold(0u32, |acc, &d| acc * 10 + (d - b'0') as u32);
    match val {
        1 => Ok(Scale::S1),
        2 => Ok(Scale::S2),
        4 => Ok(Scale::S4),
        8 => Ok(Scale::S8),
        _ => Err(EaError::InvalidScale),
    }
}

/// インデックスレジスタ指定 Rn[.w|.l][*scale] を解析する
fn try_parse_index_spec(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<Option<IndexSpec>, EaError> {
    let saved = *pos;
    skip_spaces(src, pos);
    let regno = match try_parse_register(src, pos, sym, cpu_type) {
        // Dn (0x00-0x07), An (0x08-0x0F), ZDn (0x10-0x17), ZAn (0x18-0x1F)
        Some(r) if r <= 0x1F => r,
        Some(_) => {
            *pos = saved;
            return Ok(None);
        }
        None => {
            *pos = saved;
            return Ok(None);
        }
    };
    let (idx_reg, suppress) = if regno >= 0x10 {
        (regno - 0x10, true)
    } else {
        (regno, false)
    };
    let size = try_parse_idx_size(src, pos);
    let scale = try_parse_scale(src, pos)?;
    Ok(Some(IndexSpec {
        reg: idx_reg,
        size,
        scale,
        suppress,
    }))
}

/// 括弧内でベースレジスタを得た後の解析
/// pos は base_regno を消費済みで、その直後を指している
fn parse_paren_with_base(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
    base_regno: u8,
    pre_disp: Option<Displacement>,
) -> Result<EffectiveAddress, EaError> {
    let is_pc = base_regno == reg::PC || base_regno == reg::OPC;
    let is_zpc = base_regno == reg::ZPC;
    let an = if is_pc || is_zpc {
        0 // PC ベース（reg 番号は EA フィールドには含まれない）
    } else if (0x08..=0x0F).contains(&base_regno) {
        base_regno - 0x08
    } else {
        return Err(EaError::InvalidRegister);
    };

    skip_spaces(src, pos);
    match src.get(*pos) {
        Some(&b')') => {
            *pos += 1;
            if is_pc || is_zpc {
                // (d,PC) / (PC): pre_disp があればそれを使う
                let disp = pre_disp.unwrap_or_else(Displacement::zero);
                return Ok(EffectiveAddress::PcDisp(disp));
            }
            // post-increment チェック
            skip_spaces(src, pos);
            if src.get(*pos) == Some(&b'+') {
                *pos += 1;
                if pre_disp.is_some() {
                    return Err(EaError::UnexpectedToken);
                }
                return Ok(EffectiveAddress::AddrRegPostInc(an));
            }
            if let Some(disp) = pre_disp {
                return Ok(EffectiveAddress::AddrRegDisp { an, disp });
            }
            Ok(EffectiveAddress::AddrRegInd(an))
        }
        Some(&b',') => {
            // (An, Rn...) または (d, An, Rn...) の Rn 部分
            *pos += 1;
            skip_spaces(src, pos);
            let idx = match try_parse_index_spec(src, pos, sym, cpu_type)? {
                Some(i) => i,
                None => return Err(EaError::InvalidIndexReg),
            };
            skip_spaces(src, pos);
            if src.get(*pos) != Some(&b')') {
                return Err(EaError::ExpectedCloseParen);
            }
            *pos += 1;
            let disp = pre_disp.unwrap_or_else(Displacement::zero);
            if is_pc || is_zpc {
                return Ok(EffectiveAddress::PcIdx { disp, idx });
            }
            Ok(EffectiveAddress::AddrRegIdx { an, disp, idx })
        }
        _ => Err(EaError::ExpectedCloseParen),
    }
}

/// 68020+ メモリ間接アドレッシングモード解析
/// '(' は消費済み、pos は '[' を指している
/// 形式: ([bd,An],Xn,od) / ([bd,An,Xn],od) / ([bd,PC],Xn,od) / ([bd,PC,Xn],od)
fn parse_memory_indirect(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<EffectiveAddress, EaError> {
    *pos += 1; // skip '['
    skip_spaces(src, pos);

    // '[' 内を解析: bd,An[,Xn] の形式
    // まずオプションの base displacement（式）を試す
    let mut bd = Displacement::zero();
    let mut base_regno: Option<u8> = None;
    let mut inner_idx: Option<IndexSpec> = None;

    // 最初のトークンを解析：レジスタか式か
    if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
        match regno {
            0x08..=0x0F | 0x20 | 0x2E | 0x2F => {
                // An or PC/ZPC as first token
                base_regno = Some(regno);
            }
            _ => return Err(EaError::InvalidRegister),
        }
    } else {
        // 式（base displacement）
        let rpn = parse_expr(src, pos)?;
        let size = try_parse_disp_size(src, pos);
        bd = Displacement {
            rpn,
            size,
            const_val: None,
        };
    }

    skip_spaces(src, pos);

    // ',' の後にベースレジスタやインデックスが続く可能性
    while src.get(*pos) == Some(&b',') {
        *pos += 1;
        skip_spaces(src, pos);
        if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
            match regno {
                0x08..=0x0F | 0x20 | 0x2E | 0x2F => {
                    // An or PC — ベースレジスタ
                    if base_regno.is_some() {
                        return Err(EaError::InvalidRegister);
                    }
                    base_regno = Some(regno);
                }
                0x00..=0x07 => {
                    // Dn — インデックスレジスタ（ブラケット内 = プリインデックス）
                    let size = try_parse_idx_size(src, pos);
                    let scale = try_parse_scale(src, pos)?;
                    inner_idx = Some(IndexSpec {
                        reg: regno,
                        size,
                        scale,
                        suppress: false,
                    });
                }
                _ => return Err(EaError::InvalidRegister),
            }
        } else {
            return Err(EaError::ExpectedRegister);
        }
        skip_spaces(src, pos);
    }

    // ']' を期待
    if src.get(*pos) != Some(&b']') {
        return Err(EaError::ExpectedCloseParen);
    }
    *pos += 1;
    skip_spaces(src, pos);

    let base = base_regno.ok_or(EaError::InvalidRegister)?;
    let is_pc = base == reg::PC || base == reg::OPC;
    let is_zpc = base == reg::ZPC;
    let an = if is_pc || is_zpc { 0 } else { base - 0x08 };

    // ']' の後: オプションの ',Xn,od' （ポストインデックス）または ',od'
    let mut outer_idx: Option<IndexSpec> = None;
    let mut od = Displacement::zero();

    if src.get(*pos) == Some(&b',') {
        *pos += 1;
        skip_spaces(src, pos);
        // Xn（インデックスレジスタ）を試す
        let save = *pos;
        if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
            if (0x00..=0x07).contains(&regno) {
                let size = try_parse_idx_size(src, pos);
                let scale = try_parse_scale(src, pos)?;
                outer_idx = Some(IndexSpec {
                    reg: regno,
                    size,
                    scale,
                    suppress: false,
                });
                skip_spaces(src, pos);
                // さらに ',od' があるか
                if src.get(*pos) == Some(&b',') {
                    *pos += 1;
                    skip_spaces(src, pos);
                    let rpn = parse_expr(src, pos)?;
                    let size = try_parse_disp_size(src, pos);
                    od = Displacement {
                        rpn,
                        size,
                        const_val: None,
                    };
                }
            } else {
                // レジスタだがインデックスではない → 式として再解析
                *pos = save;
                let rpn = parse_expr(src, pos)?;
                let size = try_parse_disp_size(src, pos);
                od = Displacement {
                    rpn,
                    size,
                    const_val: None,
                };
            }
        } else {
            // 式（outer displacement）
            let rpn = parse_expr(src, pos)?;
            let size = try_parse_disp_size(src, pos);
            od = Displacement {
                rpn,
                size,
                const_val: None,
            };
        }
    }

    skip_spaces(src, pos);
    // ')' を期待
    if src.get(*pos) != Some(&b')') {
        return Err(EaError::ExpectedCloseParen);
    }
    *pos += 1;

    // プリインデックス: Xn がブラケット内にある
    // ポストインデックス: Xn がブラケット外にある
    if let Some(idx) = inner_idx {
        // プリインデックス: ([bd,An,Xn],od)
        if is_pc || is_zpc {
            Ok(EffectiveAddress::PcMemIndPre { bd, idx, od })
        } else {
            Ok(EffectiveAddress::MemIndPre { an, bd, idx, od })
        }
    } else if let Some(idx) = outer_idx {
        // ポストインデックス: ([bd,An],Xn,od)
        if is_pc || is_zpc {
            Ok(EffectiveAddress::PcMemIndPost { bd, idx, od })
        } else {
            Ok(EffectiveAddress::MemIndPost { an, bd, idx, od })
        }
    } else {
        // インデックスなし（suppressed）→ プリインデックスで IS=1
        let idx = IndexSpec {
            reg: 0,
            size: IdxSize::Word,
            scale: Scale::S1,
            suppress: true,
        };
        if is_pc || is_zpc {
            Ok(EffectiveAddress::PcMemIndPre { bd, idx, od })
        } else {
            Ok(EffectiveAddress::MemIndPre { an, bd, idx, od })
        }
    }
}

/// 括弧の中を解析する（'(' は消費済み）
fn parse_paren_with_expr(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<EffectiveAddress, EaError> {
    skip_spaces(src, pos);

    // 68020+ メモリ間接: ([bd,An],Xn,od) / ([bd,An,Xn],od)
    if src.get(*pos) == Some(&b'[') {
        return parse_memory_indirect(src, pos, sym, cpu_type);
    }

    // まずレジスタ名かどうかチェック（An や PC が来たらそれがベースレジスタ）
    if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
        match regno {
            // An または PC/ZPC → ベースレジスタ
            0x08..=0x0F | 0x20 | 0x2E | 0x2F => {
                skip_spaces(src, pos);
                return parse_paren_with_base(src, pos, sym, cpu_type, regno, None);
            }
            // Dn → インデックスレジスタ（(Dn, An) 形式、ゼロベースディスプレースメント）
            0x00..=0x07 => {
                let size = try_parse_idx_size(src, pos);
                let scale = try_parse_scale(src, pos)?;
                let idx = IndexSpec {
                    reg: regno,
                    size,
                    scale,
                    suppress: false,
                };
                skip_spaces(src, pos);
                if src.get(*pos) != Some(&b',') {
                    return Err(EaError::ExpectedComma);
                }
                *pos += 1;
                skip_spaces(src, pos);
                let base_regno = match try_parse_register(src, pos, sym, cpu_type) {
                    Some(r) => r,
                    None => return Err(EaError::ExpectedRegister),
                };
                let (is_pc, is_zpc) = (
                    base_regno == reg::PC || base_regno == reg::OPC,
                    base_regno == reg::ZPC,
                );
                let an = if is_pc || is_zpc {
                    0
                } else if (0x08..=0x0F).contains(&base_regno) {
                    base_regno - 0x08
                } else {
                    return Err(EaError::InvalidRegister);
                };
                skip_spaces(src, pos);
                if src.get(*pos) != Some(&b')') {
                    return Err(EaError::ExpectedCloseParen);
                }
                *pos += 1;
                if is_pc || is_zpc {
                    return Ok(EffectiveAddress::PcIdx {
                        disp: Displacement::zero(),
                        idx,
                    });
                }
                return Ok(EffectiveAddress::AddrRegIdx {
                    an,
                    disp: Displacement::zero(),
                    idx,
                });
            }
            // 他のレジスタ（SR/CCR など）は EA として不正
            _ => return Err(EaError::InvalidRegister),
        }
    }

    // レジスタでなければ式（ディスプレースメントまたは絶対アドレス）
    let rpn = parse_expr(src, pos)?;
    let size = try_parse_disp_size(src, pos);

    skip_spaces(src, pos);
    match src.get(*pos) {
        Some(&b')') => {
            // (expr) → 絶対アドレス
            *pos += 1;
            // ($1234).w 形式のため、')' の後にもサイズ指定を確認する
            let final_size = try_parse_disp_size(src, pos).or(size);
            match final_size {
                Some(DispSize::Word) => Ok(EffectiveAddress::AbsShort(rpn)),
                Some(DispSize::Long) | None => Ok(EffectiveAddress::AbsLong(rpn)),
                Some(DispSize::Short) => Err(EaError::InvalidSize),
            }
        }
        Some(&b',') => {
            // (d, An...) 形式
            *pos += 1;
            skip_spaces(src, pos);
            let base_regno = match try_parse_register(src, pos, sym, cpu_type) {
                Some(r) => r,
                None => return Err(EaError::ExpectedRegister),
            };
            let disp = Displacement {
                rpn,
                size,
                const_val: None,
            };
            skip_spaces(src, pos);
            parse_paren_with_base(src, pos, sym, cpu_type, base_regno, Some(disp))
        }
        _ => Err(EaError::ExpectedCloseParen),
    }
}

/// レジスタ番号からマスクビット位置を返す（D0-D7=bit0-7, A0-A7=bit8-15）
fn regno_to_bit(regno: u8) -> Option<u8> {
    if regno <= 0x0F {
        Some(regno)
    } else {
        None
    }
}

/// MOVEM用レジスタリスト解析。
/// `first_regno` は既に解析済みの最初のレジスタ番号。
/// `/` または `-` が続く場合はリスト全体を解析して `Immediate(mask)` を返す。
/// 続かない場合は None を返し、`pos` は変更しない。
fn try_continue_reglist(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
    first_regno: u8,
) -> Option<EffectiveAddress> {
    let saved = *pos;
    skip_spaces(src, pos);

    let ch = src.get(*pos).copied();
    if ch != Some(b'/') && ch != Some(b'-') {
        *pos = saved;
        return None;
    }

    // レジスタリストとして解析する
    let bit = regno_to_bit(first_regno)?;
    let mut mask: u16 = 1 << bit;
    let mut last_regno = first_regno;

    loop {
        let loop_saved = *pos;
        skip_spaces(src, pos);
        match src.get(*pos).copied() {
            Some(b'/') => {
                *pos += 1;
                skip_spaces(src, pos);
                let reg_saved = *pos;
                match try_parse_register(src, pos, sym, cpu_type) {
                    Some(r) if r <= 0x0F => {
                        mask |= 1 << r;
                        last_regno = r;
                    }
                    _ => {
                        *pos = loop_saved;
                        break;
                    }
                }
                let _ = reg_saved;
            }
            Some(b'-') => {
                *pos += 1;
                skip_spaces(src, pos);
                let reg_saved = *pos;
                match try_parse_register(src, pos, sym, cpu_type) {
                    Some(r) if r <= 0x0F => {
                        // 範囲: last_regno から r まで
                        let lo = last_regno.min(r);
                        let hi = last_regno.max(r);
                        for b in lo..=hi {
                            mask |= 1 << b;
                        }
                        last_regno = r;
                    }
                    _ => {
                        // '-' の後にレジスタ名がなければ '-' の前に戻す
                        *pos = loop_saved;
                        break;
                    }
                }
                let _ = reg_saved;
            }
            _ => {
                *pos = loop_saved;
                break;
            }
        }
    }

    let rpn = vec![RPNToken::ValueWord(mask), RPNToken::End];
    Some(EffectiveAddress::Immediate(rpn))
}

/// レジスタリスト（MOVEM 用）を解析してビットマスクを返す。
///
/// `d3-d7/a2-a6` のようなレジスタリスト構文を解析し、ビットマスクを返す。
/// D0=bit0, D7=bit7, A0=bit8, A7=bit15。
/// レジスタリスト構文でなければ `pos` を変更せず `None` を返す。
pub fn parse_reg_list_mask(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Option<u16> {
    skip_spaces(src, pos);
    let saved = *pos;
    let first_regno = try_parse_register(src, pos, sym, cpu_type)?;
    if first_regno > 0x0F {
        *pos = saved;
        return None;
    }
    // / または - が続く場合は try_continue_reglist に委譲
    if let Some(EffectiveAddress::Immediate(rpn)) =
        try_continue_reglist(src, pos, sym, cpu_type, first_regno)
    {
        if let [RPNToken::ValueWord(mask), RPNToken::End] = rpn.as_slice() {
            return Some(*mask);
        }
    }
    // 単一レジスタのみ
    Some(1u16 << first_regno)
}

/// 実効アドレスを解析する
///
/// * `src`      - ソースバイト列
/// * `pos`      - 現在位置（解析後に進む）
/// * `sym`      - シンボルテーブル（レジスタ名検索用）
/// * `cpu_type` - CPU タイプビットマスク
pub fn parse_ea(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<EffectiveAddress, EaError> {
    skip_spaces(src, pos);
    if *pos >= src.len() {
        return Err(EaError::ExpectedOperand);
    }

    // #imm: イミディエイト
    if src[*pos] == b'#' {
        *pos += 1;
        skip_spaces(src, pos);
        let rpn = parse_expr(src, pos)?;
        return Ok(EffectiveAddress::Immediate(rpn));
    }

    // -(An): プリデクリメント
    if src[*pos] == b'-' {
        let saved = *pos;
        *pos += 1;
        skip_spaces(src, pos);
        if src.get(*pos) == Some(&b'(') {
            *pos += 1;
            skip_spaces(src, pos);
            let reg_pos = *pos;
            if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
                if (0x08..=0x0F).contains(&regno) {
                    let an = regno - 0x08;
                    skip_spaces(src, pos);
                    if src.get(*pos) == Some(&b')') {
                        *pos += 1;
                        return Ok(EffectiveAddress::AddrRegPreDec(an));
                    }
                }
                // An 以外か ')' がない → リセット
                let _ = reg_pos; // use variable
            }
        }
        *pos = saved;
        // 負の式として fall-through
    }

    // (…): 括弧形式
    if src[*pos] == b'(' {
        *pos += 1;
        return parse_paren_with_expr(src, pos, sym, cpu_type);
    }

    // レジスタ直接（Dn / An）、またはレジスタリスト（MOVEM用: d0/a0, d0-d7 等）
    let saved = *pos;
    if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
        match regno {
            0x00..=0x0F => {
                // レジスタリスト（/ または - が続く場合）
                if let Some(ea) = try_continue_reglist(src, pos, sym, cpu_type, regno) {
                    return Ok(ea);
                }
                match regno {
                    0x00..=0x07 => return Ok(EffectiveAddress::DataReg(regno)),
                    0x08..=0x0F => return Ok(EffectiveAddress::AddrReg(regno - 0x08)),
                    _ => unreachable!(),
                }
            }
            0x21 => return Ok(EffectiveAddress::CcrReg),
            0x22 => return Ok(EffectiveAddress::SrReg),
            _ => {}
        }
        *pos = saved; // Dn/An/CCR/SR でなければ戻す
    }

    // 式（絶対アドレスまたは displacement 前置形式）
    let rpn = parse_expr(src, pos)?;

    // .reg シンボル（RegSym）の場合は Immediate(mask) として返す
    if let [RPNToken::SymbolRef(sym_name), RPNToken::End] = rpn.as_slice() {
        if let Some(Symbol::RegSym { define }) = sym.lookup_sym(sym_name) {
            if let Some(first) = define.first() {
                if let [RPNToken::ValueWord(mask), RPNToken::End] = first.as_slice() {
                    return Ok(EffectiveAddress::Immediate(vec![
                        RPNToken::ValueWord(*mask),
                        RPNToken::End,
                    ]));
                }
            }
        }
    }

    let size = try_parse_disp_size(src, pos);

    skip_spaces(src, pos);
    if src.get(*pos) == Some(&b'(') {
        // expr(An) または expr(An,Rn) 形式
        *pos += 1;
        skip_spaces(src, pos);
        let base_regno = match try_parse_register(src, pos, sym, cpu_type) {
            Some(r) => r,
            None => return Err(EaError::ExpectedRegister),
        };
        let disp = Displacement {
            rpn,
            size,
            const_val: None,
        };
        skip_spaces(src, pos);
        return parse_paren_with_base(src, pos, sym, cpu_type, base_regno, Some(disp));
    }

    // 絶対アドレス
    match size {
        Some(DispSize::Word) => Ok(EffectiveAddress::AbsShort(rpn)),
        Some(DispSize::Long) | None => Ok(EffectiveAddress::AbsLong(rpn)),
        Some(DispSize::Short) => Err(EaError::InvalidSize),
    }
}
