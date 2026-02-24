/// 実効アドレス解析
///
/// 68000 基本12モードを実装（Phase 4）。
/// 68020+ 拡張モード（フルフォーマット、メモリ間接）は Phase 9 で追加予定。

pub mod encode;

use crate::expr::{parse_expr, ParseError as ExprParseError, Rpn};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::reg;

// ----------------------------------------------------------------
// EA モードコード定数（EAC_*、6ビットEAフィールド値）
// ----------------------------------------------------------------

/// EA モードコード定数（eamode.equ: EAC_* に対応）
pub mod eac {
    /// データレジスタ直接  Dn (000rrr)
    pub const DN:     u8 = 0b000_000;
    /// アドレスレジスタ直接 An (001rrr)
    pub const AN:     u8 = 0b001_000;
    /// アドレスレジスタ間接 (An) (010rrr)
    pub const ADR:    u8 = 0b010_000;
    /// ポストインクリメント (An)+ (011rrr)
    pub const INCADR: u8 = 0b011_000;
    /// プリデクリメント -(An) (100rrr)
    pub const DECADR: u8 = 0b100_000;
    /// ディスプレースメント付きアドレスレジスタ間接 (d16,An) (101rrr)
    pub const DSPADR: u8 = 0b101_000;
    /// インデックス付きアドレスレジスタ間接 (d8,An,Rn) (110rrr)
    pub const IDXADR: u8 = 0b110_000;
    /// 絶対ショート xxx.w (111_000 = 0o70 = 0x38)
    pub const ABSW:   u8 = 0b111_000;
    /// 絶対ロング xxx.l (111_001 = 0o71 = 0x39)
    pub const ABSL:   u8 = 0b111_001;
    /// PC相対ディスプレースメント (d16,PC) (111_010 = 0o72 = 0x3A)
    pub const DSPPC:  u8 = 0b111_010;
    /// PC相対インデックス (d8,PC,Rn) (111_011 = 0o73 = 0x3B)
    pub const IDXPC:  u8 = 0b111_011;
    /// イミディエイト #imm (111_100 = 0o74 = 0x3C)
    pub const IMM:    u8 = 0b111_100;
}

// ----------------------------------------------------------------
// EA モードビットマスク
// ----------------------------------------------------------------

/// EA モードビットマスク（eamode.equ: EA_* に対応）
pub mod ea {
    pub const DN:     u16 = 1 << 0;
    pub const AN:     u16 = 1 << 1;
    pub const ADR:    u16 = 1 << 2;
    pub const INCADR: u16 = 1 << 3;
    pub const DECADR: u16 = 1 << 4;
    pub const DSPADR: u16 = 1 << 5;
    pub const IDXADR: u16 = 1 << 6;
    pub const ABSW:   u16 = 1 << 7;
    pub const ABSL:   u16 = 1 << 8;
    pub const DSPPC:  u16 = 1 << 9;
    pub const IDXPC:  u16 = 1 << 10;
    pub const IMM:    u16 = 1 << 11;
    /// データモード（An と #imm 以外の全モード）
    pub const DATA: u16 = DN|ADR|INCADR|DECADR|DSPADR|IDXADR|ABSW|ABSL|DSPPC|IDXPC|IMM;
    /// メモリモード（Dn/An 以外）
    pub const MEM:  u16 = ADR|INCADR|DECADR|DSPADR|IDXADR|ABSW|ABSL|DSPPC|IDXPC|IMM;
    /// 変更可能モード（PC相対 / #imm 以外）
    pub const ALT:  u16 = DN|AN|ADR|INCADR|DECADR|DSPADR|IDXADR|ABSW|ABSL;
    /// 制御モード
    pub const CTRL: u16 = ADR|DSPADR|IDXADR|ABSW|ABSL|DSPPC|IDXPC;
    /// 全モード
    pub const ALL:  u16 = DN|AN|ADR|INCADR|DECADR|DSPADR|IDXADR|ABSW|ABSL|DSPPC|IDXPC|IMM;
}

// ----------------------------------------------------------------
// 型定義
// ----------------------------------------------------------------

/// ディスプレースメントのサイズ指定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispSize {
    Short,  // .s（brief format インデックスディスプレースメント、8ビット）
    Word,   // .w（16ビット）
    Long,   // .l（32ビット）
}

/// ディスプレースメント式
#[derive(Debug, Clone)]
pub struct Displacement {
    /// RPN式（空の Vec = ゼロディスプレースメント）
    pub rpn: Rpn,
    /// サイズ指定（None = 自動）
    pub size: Option<DispSize>,
    /// 定数値（解析時に評価できた場合）
    pub const_val: Option<i32>,
}

impl Displacement {
    /// ゼロディスプレースメント
    pub fn zero() -> Self {
        Displacement { rpn: vec![], size: None, const_val: Some(0) }
    }

    /// 定数かどうか
    pub fn is_const(&self) -> bool {
        self.const_val.is_some()
    }

    /// ゼロかどうか（定数かつ値が0）
    pub fn is_zero(&self) -> bool {
        self.const_val == Some(0)
    }
}

/// インデックスレジスタのワード/ロング指定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdxSize {
    Word,  // .w（デフォルト）
    Long,  // .l
}

/// スケールファクタ（68000 では *1 のみ有効、68020+ では *2/*4/*8 も可）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    S1 = 0,
    S2 = 1,
    S4 = 2,
    S8 = 3,
}

/// インデックスレジスタ指定
#[derive(Debug, Clone)]
pub struct IndexSpec {
    /// 0-7: Dn、8-15: An
    pub reg: u8,
    pub size: IdxSize,
    pub scale: Scale,
    /// レジスタサプレス（ZDn/ZAn、68020+ のみ）
    pub suppress: bool,
}

/// 実効アドレス
#[derive(Debug, Clone)]
pub enum EffectiveAddress {
    /// データレジスタ直接 Dn（n: 0-7）
    DataReg(u8),
    /// アドレスレジスタ直接 An（n: 0-7）
    AddrReg(u8),
    /// アドレスレジスタ間接 (An)
    AddrRegInd(u8),
    /// ポストインクリメント (An)+
    AddrRegPostInc(u8),
    /// プリデクリメント -(An)
    AddrRegPreDec(u8),
    /// ディスプレースメント付きアドレスレジスタ間接 (d16,An) / d16(An)
    AddrRegDisp { an: u8, disp: Displacement },
    /// インデックス付きアドレスレジスタ間接 (d8,An,Rn) / d8(An,Rn)
    AddrRegIdx { an: u8, disp: Displacement, idx: IndexSpec },
    /// 絶対ショートアドレス xxx.w
    AbsShort(Rpn),
    /// 絶対ロングアドレス xxx.l / xxx（デフォルト）
    AbsLong(Rpn),
    /// PC相対ディスプレースメント (d16,PC)
    PcDisp(Displacement),
    /// PC相対インデックス (d8,PC,Rn)
    PcIdx { disp: Displacement, idx: IndexSpec },
    /// イミディエイト #imm
    Immediate(Rpn),
}

impl EffectiveAddress {
    /// EA ビットマスクを返す
    pub fn ea_bits(&self) -> u16 {
        match self {
            Self::DataReg(_)        => ea::DN,
            Self::AddrReg(_)        => ea::AN,
            Self::AddrRegInd(_)     => ea::ADR,
            Self::AddrRegPostInc(_) => ea::INCADR,
            Self::AddrRegPreDec(_)  => ea::DECADR,
            Self::AddrRegDisp { .. }=> ea::DSPADR,
            Self::AddrRegIdx { .. } => ea::IDXADR,
            Self::AbsShort(_)       => ea::ABSW,
            Self::AbsLong(_)        => ea::ABSL,
            Self::PcDisp(_)         => ea::DSPPC,
            Self::PcIdx { .. }      => ea::IDXPC,
            Self::Immediate(_)      => ea::IMM,
        }
    }
}

// ----------------------------------------------------------------
// エラー型
// ----------------------------------------------------------------

/// EA パースエラー
#[derive(Debug, Clone, PartialEq)]
pub enum EaError {
    /// オペランドが見つからない
    ExpectedOperand,
    /// ')' が必要
    ExpectedCloseParen,
    /// ',' が必要
    ExpectedComma,
    /// レジスタが必要
    ExpectedRegister,
    /// レジスタが不正（An が必要な位置に Dn 等）
    InvalidRegister,
    /// 不正なサイズ指定
    InvalidSize,
    /// 不正なスケール値（1/2/4/8 以外）
    InvalidScale,
    /// 不正なインデックスレジスタ（Dn/An のみ）
    InvalidIndexReg,
    /// 予期しないトークン
    UnexpectedToken,
    /// 式解析エラー
    ExprError(ExprParseError),
}

impl From<ExprParseError> for EaError {
    fn from(e: ExprParseError) -> Self {
        EaError::ExprError(e)
    }
}

// ----------------------------------------------------------------
// パーサー内部ユーティリティ
// ----------------------------------------------------------------

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
fn try_parse_register(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Option<u8> {
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
        b's' => { *pos += 1; Some(DispSize::Short) }
        b'w' => { *pos += 1; Some(DispSize::Word) }
        b'l' => { *pos += 1; Some(DispSize::Long) }
        _ => { *pos = saved; None }
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
        b'w' => { *pos += 1; IdxSize::Word }
        b'l' => { *pos += 1; IdxSize::Long }
        _ => { *pos = saved; IdxSize::Word }
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
    let val: u32 = src[start..*pos].iter()
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
    Ok(Some(IndexSpec { reg: idx_reg, size, scale, suppress }))
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
    let is_pc  = base_regno == reg::PC || base_regno == reg::OPC;
    let is_zpc = base_regno == reg::ZPC;
    let an = if is_pc || is_zpc {
        0 // PC ベース（reg 番号は EA フィールドには含まれない）
    } else if base_regno >= 0x08 && base_regno <= 0x0F {
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

/// 括弧の中を解析する（'(' は消費済み）
fn parse_paren_with_expr(
    src: &[u8],
    pos: &mut usize,
    sym: &SymbolTable,
    cpu_type: u16,
) -> Result<EffectiveAddress, EaError> {
    skip_spaces(src, pos);

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
                let idx = IndexSpec { reg: regno, size, scale, suppress: false };
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
                let (is_pc, is_zpc) = (base_regno == reg::PC || base_regno == reg::OPC, base_regno == reg::ZPC);
                let an = if is_pc || is_zpc {
                    0
                } else if base_regno >= 0x08 && base_regno <= 0x0F {
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
                    return Ok(EffectiveAddress::PcIdx { disp: Displacement::zero(), idx });
                }
                return Ok(EffectiveAddress::AddrRegIdx { an, disp: Displacement::zero(), idx });
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
            let disp = Displacement { rpn, size: size.map(|s| s), const_val: None };
            skip_spaces(src, pos);
            parse_paren_with_base(src, pos, sym, cpu_type, base_regno, Some(disp))
        }
        _ => Err(EaError::ExpectedCloseParen),
    }
}

// ----------------------------------------------------------------
// 公開 API
// ----------------------------------------------------------------

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
                if regno >= 0x08 && regno <= 0x0F {
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

    // レジスタ直接（Dn / An）
    let saved = *pos;
    if let Some(regno) = try_parse_register(src, pos, sym, cpu_type) {
        match regno {
            0x00..=0x07 => return Ok(EffectiveAddress::DataReg(regno)),
            0x08..=0x0F => return Ok(EffectiveAddress::AddrReg(regno - 0x08)),
            _ => {}
        }
        *pos = saved; // Dn/An でなければ戻す
    }

    // 式（絶対アドレスまたは displacement 前置形式）
    let rpn = parse_expr(src, pos)?;
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
        let disp = Displacement { rpn, size: size.map(|s| s), const_val: None };
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

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol::SymbolTable;
    use crate::options::cpu;

    fn make_sym() -> SymbolTable {
        SymbolTable::new(false)
    }

    fn parse(s: &str) -> EffectiveAddress {
        let sym = make_sym();
        let mut pos = 0;
        parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect(s)
    }

    fn parse_err(s: &str) -> EaError {
        let sym = make_sym();
        let mut pos = 0;
        parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect_err(s)
    }

    // ---- レジスタ直接 ----

    #[test]
    fn test_data_reg() {
        for i in 0..8u8 {
            let s = format!("d{}", i);
            assert_eq!(parse(&s), EffectiveAddress::DataReg(i), "{}", s);
        }
        assert_eq!(parse("D3"), EffectiveAddress::DataReg(3));
    }

    #[test]
    fn test_addr_reg() {
        for i in 0..8u8 {
            let s = format!("a{}", i);
            assert_eq!(parse(&s), EffectiveAddress::AddrReg(i), "{}", s);
        }
        assert_eq!(parse("sp"), EffectiveAddress::AddrReg(7));
        assert_eq!(parse("SP"), EffectiveAddress::AddrReg(7));
    }

    // ---- アドレスレジスタ間接 ----

    #[test]
    fn test_addr_reg_ind() {
        assert_eq!(parse("(a0)"), EffectiveAddress::AddrRegInd(0));
        assert_eq!(parse("(a5)"), EffectiveAddress::AddrRegInd(5));
        assert_eq!(parse("( a0 )"), EffectiveAddress::AddrRegInd(0));
    }

    #[test]
    fn test_post_inc() {
        assert_eq!(parse("(a0)+"), EffectiveAddress::AddrRegPostInc(0));
        assert_eq!(parse("(a7)+"), EffectiveAddress::AddrRegPostInc(7));
    }

    #[test]
    fn test_pre_dec() {
        assert_eq!(parse("-(a0)"), EffectiveAddress::AddrRegPreDec(0));
        assert_eq!(parse("-(sp)"), EffectiveAddress::AddrRegPreDec(7));
    }

    // ---- ディスプレースメント付きアドレスレジスタ間接 ----

    fn disp_val(ea: &EffectiveAddress) -> i32 {
        match ea {
            EffectiveAddress::AddrRegDisp { disp, .. } => {
                // RPN を評価して定数を得る
                crate::expr::eval_rpn(&disp.rpn, 0, 0, 0, &|_| None).unwrap().value
            }
            _ => panic!("not AddrRegDisp"),
        }
    }

    fn disp_an(ea: &EffectiveAddress) -> u8 {
        match ea {
            EffectiveAddress::AddrRegDisp { an, .. } => *an,
            _ => panic!("not AddrRegDisp"),
        }
    }

    #[test]
    fn test_addr_reg_disp() {
        // 括弧内形式 (d,An)
        let ea = parse("(4,a0)");
        assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 0, .. }));
        assert_eq!(disp_val(&ea), 4);

        // 前置形式 d(An)
        let ea2 = parse("4(a0)");
        assert!(matches!(ea2, EffectiveAddress::AddrRegDisp { an: 0, .. }));
        assert_eq!(disp_val(&ea2), 4);

        // 負のディスプレースメント
        let ea3 = parse("(-8,a5)");
        assert_eq!(disp_an(&ea3), 5);
        assert_eq!(disp_val(&ea3), -8);
    }

    #[test]
    fn test_addr_reg_disp_zero() {
        // (0,An) は (An) と同じではなく AddrRegDisp として解析される
        let ea = parse("(0,a3)");
        assert!(matches!(ea, EffectiveAddress::AddrRegDisp { an: 3, .. }));
    }

    // ---- インデックス付きアドレスレジスタ間接 ----

    #[test]
    fn test_addr_reg_idx_basic() {
        // (0,a0,d1) → AddrRegIdx
        let ea = parse("(0,a0,d1)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 0);
                assert_eq!(idx.reg, 1);
                assert_eq!(idx.size, IdxSize::Word);
                assert_eq!(idx.scale, Scale::S1);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_long() {
        let ea = parse("(2,a3,d4.l)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 3);
                assert_eq!(idx.reg, 4);
                assert_eq!(idx.size, IdxSize::Long);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_an_index() {
        // インデックスレジスタに An を使う
        let ea = parse("(0,a0,a1.w)");
        match ea {
            EffectiveAddress::AddrRegIdx { an, ref idx, .. } => {
                assert_eq!(an, 0);
                assert_eq!(idx.reg, 0x08 + 1); // A1
                assert_eq!(idx.size, IdxSize::Word);
            }
            _ => panic!("expected AddrRegIdx"),
        }
    }

    #[test]
    fn test_addr_reg_idx_no_disp() {
        // (a0,d1) → AddrRegIdx with zero displacement
        let ea = parse("(a0,d1)");
        match &ea {
            EffectiveAddress::AddrRegIdx { an, disp, idx } => {
                assert_eq!(*an, 0);
                assert!(disp.is_zero());
                assert_eq!(idx.reg, 1);
            }
            _ => panic!("expected AddrRegIdx, got {:?}", ea),
        }
    }

    #[test]
    fn test_addr_reg_idx_dn_first() {
        // (d1,a0) → AddrRegIdx (Dn が先でも An がベース)
        let ea = parse("(d1,a0)");
        match &ea {
            EffectiveAddress::AddrRegIdx { an, disp, idx } => {
                assert_eq!(*an, 0);
                assert!(disp.is_zero());
                assert_eq!(idx.reg, 1);
            }
            _ => panic!("expected AddrRegIdx, got {:?}", ea),
        }
    }

    // ---- 絶対アドレス ----

    #[test]
    fn test_abs_short() {
        let ea = parse("$1234.w");
        assert!(matches!(ea, EffectiveAddress::AbsShort(_)));

        let ea2 = parse("($1234).w");
        assert!(matches!(ea2, EffectiveAddress::AbsShort(_)));
    }

    #[test]
    fn test_abs_long() {
        let ea = parse("$12345678.l");
        assert!(matches!(ea, EffectiveAddress::AbsLong(_)));

        // デフォルト（サイズ指定なし）はロング
        let ea2 = parse("$1000");
        assert!(matches!(ea2, EffectiveAddress::AbsLong(_)));

        let ea3 = parse("($1234).l");
        assert!(matches!(ea3, EffectiveAddress::AbsLong(_)));
    }

    // ---- PC相対 ----

    #[test]
    fn test_pc_disp() {
        let ea = parse("(4,pc)");
        assert!(matches!(ea, EffectiveAddress::PcDisp(_)));
    }

    #[test]
    fn test_pc_idx() {
        let ea = parse("(2,pc,d0)");
        assert!(matches!(ea, EffectiveAddress::PcIdx { .. }));
    }

    // ---- イミディエイト ----

    #[test]
    fn test_immediate() {
        let ea = parse("#100");
        assert!(matches!(ea, EffectiveAddress::Immediate(_)));
    }

    #[test]
    fn test_immediate_hex() {
        let ea = parse("#$FFFF");
        assert!(matches!(ea, EffectiveAddress::Immediate(_)));
    }

    // ---- EA ビットマスク ----

    #[test]
    fn test_ea_bits() {
        assert_eq!(parse("d0").ea_bits(), ea::DN);
        assert_eq!(parse("a0").ea_bits(), ea::AN);
        assert_eq!(parse("(a0)").ea_bits(), ea::ADR);
        assert_eq!(parse("(a0)+").ea_bits(), ea::INCADR);
        assert_eq!(parse("-(a0)").ea_bits(), ea::DECADR);
        assert_eq!(parse("(4,a0)").ea_bits(), ea::DSPADR);
        assert_eq!(parse("(0,a0,d0)").ea_bits(), ea::IDXADR);
        assert_eq!(parse("$1000.w").ea_bits(), ea::ABSW);
        assert_eq!(parse("$1000").ea_bits(), ea::ABSL);
        assert_eq!(parse("(4,pc)").ea_bits(), ea::DSPPC);
        assert_eq!(parse("(0,pc,d0)").ea_bits(), ea::IDXPC);
        assert_eq!(parse("#0").ea_bits(), ea::IMM);
    }

    // ---- エラーケース ----

    #[test]
    fn test_error_empty() {
        assert_eq!(parse_err(""), EaError::ExpectedOperand);
    }
}

// PartialEq のための実装（テスト用）
impl PartialEq for EffectiveAddress {
    fn eq(&self, other: &Self) -> bool {
        use EffectiveAddress::*;
        match (self, other) {
            (DataReg(a),        DataReg(b))        => a == b,
            (AddrReg(a),        AddrReg(b))        => a == b,
            (AddrRegInd(a),     AddrRegInd(b))     => a == b,
            (AddrRegPostInc(a), AddrRegPostInc(b)) => a == b,
            (AddrRegPreDec(a),  AddrRegPreDec(b))  => a == b,
            (AddrRegDisp { an: a, .. }, AddrRegDisp { an: b, .. }) => a == b,
            (AddrRegIdx { an: a, .. },  AddrRegIdx { an: b, .. })  => a == b,
            (AbsShort(_),  AbsShort(_))  => true,
            (AbsLong(_),   AbsLong(_))   => true,
            (PcDisp(_),    PcDisp(_))    => true,
            (PcIdx { .. }, PcIdx { .. }) => true,
            (Immediate(_), Immediate(_)) => true,
            _ => false,
        }
    }
}
