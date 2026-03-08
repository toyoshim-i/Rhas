//! Pseudo-instruction handlers for data directives
//!
//! Handles: .dc (declare constant), .ds (declare space), .dcb (declare block)
//! These directives define data in the current section.

use crate::expr::{parse_expr, Rpn};
use crate::source::SourceStack;
use crate::symbol::types::{InsnHandler, SizeCode};
use super::super::temp::TempRecord;
use crate::pass::pass1::{P1Ctx, skip_spaces};

/// Size code to byte count conversion
fn size_to_bytes(size: Option<SizeCode>) -> u32 {
    match size {
        Some(SizeCode::Byte) => 1,
        Some(SizeCode::Long) => 4,
        None | Some(SizeCode::Word) => 2,
        _ => 2,
    }
}

/// Parse a .dc directive body (used internally)
fn parse_dc(
    line: &[u8],
    pos: &mut usize,
    byte_size: u8,
    records: &mut Vec<TempRecord>,
    p1: &mut P1Ctx<'_>,
) {
    skip_spaces(line, pos);
    loop {
        if *pos >= line.len() || line[*pos] == b';' { break; }

        // 単一引用符の文字列リテラル '...' がスタンドアロン（次が , ; EOL）の場合
        // HAS 互換: .dc.b 'd0' → 全バイトを出力（文字列モード）
        // .dc.w 'AB' → 2文字を1ワードにパック, .dc.l も同様
        // 式の一部（'A'+1 等）は式として評価（スタンドアロンでない場合）
        if line[*pos] == b'\'' {
            // 文字列を抽出してスタンドアロンか確認
            let saved_pos = *pos;
            *pos += 1; // opening '
            let mut s: Vec<u8> = Vec::new();
            let mut valid = false;
            while *pos < line.len() {
                if line[*pos] == b'\'' {
                    *pos += 1; // closing '
                    // 次の文字がスタンドアロン境界か確認（スペース、,、;、EOL）
                    let mut check = *pos;
                    while check < line.len() && (line[check] == b' ' || line[check] == b'\t') {
                        check += 1;
                    }
                    if check >= line.len() || line[check] == b',' || line[check] == b';' {
                        valid = true;
                    }
                    break;
                }
                // Shift_JIS の 2 バイト文字を考慮：先行バイトなら次のバイトも取り込む
                let b = line[*pos];
                *pos += 1;
                s.push(b);
                if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
                    if *pos < line.len() {
                        s.push(line[*pos]);
                        *pos += 1;
                    }
                }
            }
            if valid && !s.is_empty() {
                match byte_size {
                    1 => {
                        p1.advance(s.len() as u32);
                        records.push(TempRecord::Const(s));
                    }
                    2 => {
                        let mut bytes = Vec::new();
                        let mut i = 0;
                        while i < s.len() {
                            if i + 1 < s.len() {
                                bytes.push(s[i]);
                                bytes.push(s[i+1]);
                                i += 2;
                            } else {
                                bytes.push(0);
                                bytes.push(s[i]);
                                i += 1;
                            }
                        }
                        p1.advance(bytes.len() as u32);
                        records.push(TempRecord::Const(bytes));
                    }
                    4 => {
                        let mut bytes = Vec::new();
                        let mut i = 0;
                        while i < s.len() {
                            let remaining = s.len() - i;
                            let pad = 4usize.saturating_sub(remaining);
                            bytes.extend(std::iter::repeat_n(0u8, pad));
                            let take = remaining.min(4);
                            bytes.extend_from_slice(&s[i..i+take]);
                            i += take;
                        }
                        p1.advance(bytes.len() as u32);
                        records.push(TempRecord::Const(bytes));
                    }
                    _ => {}
                }
                // カンマ区切りへ続く
                skip_spaces(line, pos);
                if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                } else {
                    break;
                }
                continue;
            } else {
                // スタンドアロンでない → posをリセットして式として評価
                *pos = saved_pos;
            }
        }

        // 文字列リテラル "..." → バイト列として埋め込む
        if line[*pos] == b'"' {
            *pos += 1;
            let mut s = Vec::new();
            while *pos < line.len() && line[*pos] != b'"' {
                s.push(line[*pos]);
                *pos += 1;
            }
            if *pos < line.len() { *pos += 1; } // closing "
            match byte_size {
                1 => {
                    p1.advance(s.len() as u32);
                    records.push(TempRecord::Const(s));
                }
                2 => {
                    let mut bytes = Vec::with_capacity(s.len() * 2);
                    for b in &s { bytes.push(0); bytes.push(*b); }
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                }
                4 => {
                    let mut bytes = Vec::with_capacity(s.len() * 4);
                    for b in &s { bytes.push(0); bytes.push(0); bytes.push(0); bytes.push(*b); }
                    p1.advance(bytes.len() as u32);
                    records.push(TempRecord::Const(bytes));
                }
                _ => {}
            }
        } else {
            // 式
            match parse_expr(line, pos) {
                Ok(rpn) => {
                    // .reg シンボル（RegSym）の展開チェック
                    let regsym_elems: Option<Vec<Rpn>> = {
                        if let [crate::expr::rpn::RPNToken::SymbolRef(sym_name), crate::expr::rpn::RPNToken::End] = rpn.as_slice() {
                            match p1.sym.lookup_sym(sym_name) {
                                Some(crate::symbol::Symbol::RegSym { define }) => Some(define.clone()),
                                _ => None,
                            }
                        } else { None }
                    };
                    if let Some(elem_rpns) = regsym_elems {
                        for elem_rpn in &elem_rpns {
                            if is_literal_only_rpn(elem_rpn) {
                                if let Some(v) = p1.eval_const(elem_rpn) {
                                    let bytes = val_to_bytes(v.value, byte_size);
                                    p1.advance(bytes.len() as u32);
                                    records.push(TempRecord::Const(bytes));
                                    continue;
                                }
                            }
                            {
                                p1.advance(byte_size as u32);
                                records.push(TempRecord::Data { size: byte_size, rpn: elem_rpn.clone() });
                            }
                        }
                    } else if is_literal_only_rpn(&rpn) {
                        if let Some(v) = p1.eval_const(&rpn) {
                            let bytes = val_to_bytes(v.value, byte_size);
                            p1.advance(bytes.len() as u32);
                            records.push(TempRecord::Const(bytes));
                        } else {
                            p1.advance(byte_size as u32);
                            records.push(TempRecord::Data { size: byte_size, rpn });
                        }
                    } else {
                        p1.advance(byte_size as u32);
                        records.push(TempRecord::Data { size: byte_size, rpn });
                    }
                }
                Err(_) => break,
            }
        }

        // カンマ区切り
        skip_spaces(line, pos);
        if *pos < line.len() && line[*pos] == b',' {
            *pos += 1;
            skip_spaces(line, pos);
        } else {
            break;
        }
    }
}

fn val_to_bytes(v: i32, size: u8) -> Vec<u8> {
    match size {
        1 => vec![v as u8],
        2 => { let w = v as u16; vec![(w >> 8) as u8, w as u8] }
        4 => {
            let l = v as u32;
            vec![(l>>24) as u8, (l>>16) as u8, (l>>8) as u8, l as u8]
        }
        _ => vec![],
    }
}

fn is_literal_only_rpn(rpn: &Rpn) -> bool {
    use crate::expr::rpn::RPNToken;
    rpn.iter().all(|tok| matches!(
        tok,
        RPNToken::ValueByte(_)
        | RPNToken::ValueWord(_)
        | RPNToken::Value(_)
        | RPNToken::Op(_)
        | RPNToken::End
    ))
}

/// Handle data definition directive (.dc, .ds, .dcb)
pub fn handle_data(
    handler: InsnHandler,
    size: Option<SizeCode>,
    line: &[u8],
    pos: &mut usize,
    p1: &mut P1Ctx<'_>,
    records: &mut Vec<TempRecord>,
    _source: &mut SourceStack,
) {
    match handler {
        InsnHandler::Dc => {
            if !p1.is_offset_mode() {
                let byte_size: u8 = match size {
                    Some(SizeCode::Byte) => 1,
                    Some(SizeCode::Long) => 4,
                    None | Some(SizeCode::Word) => 2,
                    _ => 2,
                };
                parse_dc(line, pos, byte_size, records, p1);
            }
        }
        InsnHandler::Ds => {
            let item_size: u32 = size_to_bytes(size);
            skip_spaces(line, pos);
            if let Ok(rpn) = parse_expr(line, pos) {
                if let Some(v) = p1.eval_const(&rpn) {
                    let count = v.value as u32;
                    let byte_count = count * item_size;
                    p1.advance(byte_count);
                    if !p1.is_offset_mode() {
                        records.push(TempRecord::Ds { byte_count });
                    }
                }
            }
        }
        InsnHandler::Dcb => {
            let item_size: u32 = size_to_bytes(size);
            skip_spaces(line, pos);
            if let Ok(count_rpn) = parse_expr(line, pos) {
                let count = p1.eval_const(&count_rpn).map(|v| v.value as u32).unwrap_or(0);
                skip_spaces(line, pos);
                let fill = if *pos < line.len() && line[*pos] == b',' {
                    *pos += 1;
                    skip_spaces(line, pos);
                    let mut fill_bytes = vec![0u8; item_size as usize];
                    if let Ok(rpn) = parse_expr(line, pos) {
                        if let Some(v) = p1.eval_const(&rpn) {
                            match item_size {
                                1 => fill_bytes[0] = v.value as u8,
                                2 => {
                                    let w = v.value as u16;
                                    fill_bytes[0] = (w >> 8) as u8;
                                    fill_bytes[1] = w as u8;
                                }
                                4 => {
                                    let l = v.value as u32;
                                    fill_bytes[0] = (l >> 24) as u8;
                                    fill_bytes[1] = (l >> 16) as u8;
                                    fill_bytes[2] = (l >> 8) as u8;
                                    fill_bytes[3] = l as u8;
                                }
                                _ => {}
                            }
                        }
                    }
                    fill_bytes
                } else {
                    vec![0u8; item_size as usize]
                };
                let mut all = Vec::with_capacity((count * item_size) as usize);
                for _ in 0..count { all.extend_from_slice(&fill); }
                let len = all.len() as u32;
                p1.advance(len);
                records.push(TempRecord::Const(all));
            }
        }
        _ => {}
    }
}
