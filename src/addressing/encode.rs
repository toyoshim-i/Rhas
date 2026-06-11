//! 実効アドレスエンコード
//!
//! `EffectiveAddress` → 6ビット EA フィールド + 拡張ワードバイト列

use super::{eac, Displacement, EffectiveAddress, IdxSize, IndexSpec};
use crate::expr::eval_rpn;

// ----------------------------------------------------------------
// 型定義
// ----------------------------------------------------------------

/// EA エンコード結果
#[derive(Debug, Clone)]
pub struct EaEncoded {
    /// 6ビット EA フィールド（instruction word の EA 部にORする）
    pub ea_field: u8,
    /// 拡張ワード・拡張データ（big-endian バイト列）
    pub ext_bytes: Vec<u8>,
}

impl EaEncoded {
    fn new(ea_field: u8) -> Self {
        EaEncoded {
            ea_field,
            ext_bytes: Vec::new(),
        }
    }

    fn push_word(&mut self, w: u16) {
        self.ext_bytes.push((w >> 8) as u8);
        self.ext_bytes.push(w as u8);
    }

    fn push_long(&mut self, l: u32) {
        self.ext_bytes.push((l >> 24) as u8);
        self.ext_bytes.push((l >> 16) as u8);
        self.ext_bytes.push((l >> 8) as u8);
        self.ext_bytes.push(l as u8);
    }
}

/// EA エンコードエラー
#[derive(Debug, Clone, PartialEq)]
pub enum EncodeError {
    /// シンボル参照を含む（Pass 7 で解決）
    DeferToLinker,
    /// ディスプレースメントが範囲外
    DisplacementOutOfRange { value: i32, bits: u8 },
    /// 不正なモード
    InvalidMode,
}

// ----------------------------------------------------------------
// 内部ユーティリティ
// ----------------------------------------------------------------

/// ディスプレースメント式を評価する（定数のみ対応）
fn eval_disp(disp: &Displacement) -> Result<i32, EncodeError> {
    if let Some(v) = disp.const_val {
        return Ok(v);
    }
    if disp.rpn.is_empty() {
        return Ok(0);
    }
    match eval_rpn(&disp.rpn, 0, 0, 0, &|_| None) {
        Ok(v) if v.section == 0 => Ok(v.value),
        _ => Err(EncodeError::DeferToLinker),
    }
}

/// RPN 式を評価する（定数のみ対応）
fn eval_rpn_const(rpn: &crate::expr::Rpn) -> Result<i32, EncodeError> {
    if rpn.is_empty() {
        return Ok(0);
    }
    match eval_rpn(rpn, 0, 0, 0, &|_| None) {
        Ok(v) if v.section == 0 => Ok(v.value),
        _ => Err(EncodeError::DeferToLinker),
    }
}

/// インデックスレジスタの brief 拡張ワードを作る（ brief format、68000/68020 共通）
///
/// brief extension word format:
/// ```text
/// bit 15:    register type (0=Dn, 1=An)
/// bits 14-12: register number (0-7)
/// bit 11:    index size (0=.w, 1=.l)
/// bits 10-9: scale (00=*1, 01=*2, 10=*4, 11=*8)
/// bit 8:     0 (brief format identifier)
/// bits 7-0:  displacement (signed 8-bit)
/// ```
fn make_brief_ext(idx: &IndexSpec, disp8: i8) -> u16 {
    let da_bit = if idx.reg >= 8 { 1u16 } else { 0u16 };
    let reg_num = (idx.reg & 7) as u16;
    let sz_bit = if idx.size == IdxSize::Long {
        1u16
    } else {
        0u16
    };
    let scale = idx.scale as u16;
    (da_bit << 15) | (reg_num << 12) | (sz_bit << 11) | (scale << 9) | ((disp8 as u8) as u16)
}

/// 16ビット符号付き範囲チェック
fn check_word(v: i32) -> Result<i16, EncodeError> {
    if v >= i16::MIN as i32 && v <= i16::MAX as i32 {
        Ok(v as i16)
    } else {
        Err(EncodeError::DisplacementOutOfRange { value: v, bits: 16 })
    }
}

/// 8ビット符号付き範囲チェック
fn check_byte(v: i32) -> Result<i8, EncodeError> {
    if v >= i8::MIN as i32 && v <= i8::MAX as i32 {
        Ok(v as i8)
    } else {
        Err(EncodeError::DisplacementOutOfRange { value: v, bits: 8 })
    }
}

/// 68020+ フルフォーマット拡張ワードを作る
///
/// full format extension word:
/// ```text
/// bit 15:    D/A (0=Dn, 1=An for index register)
/// bits 14-12: index register number (0-7)
/// bit 11:    W/L (0=word, 1=long index size)
/// bits 10-9: scale (00=*1, 01=*2, 10=*4, 11=*8)
/// bit 8:     1 (full format identifier)
/// bit 7:     BS (base suppress)
/// bit 6:     IS (index suppress)
/// bits 5-4:  BD size (01=null, 10=word, 11=long)
/// bit 3:     0
/// bits 2-0:  I/IS (indirect/index select)
/// ```
fn make_full_ext(idx: &IndexSpec, bs: bool, bd_size: u8, iis: u8) -> u16 {
    let da_bit = if idx.reg >= 8 { 1u16 } else { 0u16 };
    let reg_num = (idx.reg & 7) as u16;
    let sz_bit = if idx.size == IdxSize::Long {
        1u16
    } else {
        0u16
    };
    let scale = idx.scale as u16;
    let is_bit = if idx.suppress { 1u16 } else { 0u16 };
    let bs_bit = if bs { 1u16 } else { 0u16 };
    (da_bit << 15) | (reg_num << 12) | (sz_bit << 11) | (scale << 9)
        | 0x0100  // bit 8 = full format
        | (bs_bit << 7) | (is_bit << 6)
        | ((bd_size as u16) << 4)
        | (iis as u16)
}

/// ディスプレースメントの BD/OD サイズを決定する（01=null, 10=word, 11=long）
fn disp_bd_size(v: i32) -> u8 {
    if v == 0 {
        1
    }
    // null
    else if v >= i16::MIN as i32 && v <= i16::MAX as i32 {
        2
    }
    // word
    else {
        3
    } // long
}

/// メモリ間接アドレッシングのエンコード共通処理
fn encode_mem_indirect(
    enc: &mut EaEncoded,
    idx: &IndexSpec,
    bd: &Displacement,
    od: &Displacement,
    is_post: bool,
) -> Result<(), EncodeError> {
    let bd_val = eval_disp(bd)?;
    let od_val = eval_disp(od)?;
    let bd_sz = disp_bd_size(bd_val);
    let od_sz = disp_bd_size(od_val);

    // I/IS encoding:
    // Postindexed:  101=null_od, 110=word_od, 111=long_od
    // Preindexed:   001=null_od, 010=word_od, 011=long_od
    let iis = if is_post {
        0b100 | od_sz // 101/110/111
    } else {
        od_sz // 001/010/011
    };

    let ext = make_full_ext(idx, false, bd_sz, iis);
    enc.push_word(ext);

    // Base displacement
    match bd_sz {
        2 => enc.push_word(bd_val as u16),
        3 => enc.push_long(bd_val as u32),
        _ => {} // null
    }
    // Outer displacement
    match od_sz {
        2 => enc.push_word(od_val as u16),
        3 => enc.push_long(od_val as u32),
        _ => {} // null
    }
    Ok(())
}

// ----------------------------------------------------------------
// 公開 API
// ----------------------------------------------------------------

/// 実効アドレスをエンコードする
///
/// * `ea`       - 実効アドレス
/// * `op_size`  - オペレーションサイズ（#imm のバイト数用: 0=byte,1=word,2=long）
///
/// 戻り値: `EaEncoded`（ea_field と拡張ワードバイト列）
pub fn encode_ea(ea: &EffectiveAddress, op_size: u8) -> Result<EaEncoded, EncodeError> {
    match ea {
        // ---- 拡張ワードなし ----
        EffectiveAddress::DataReg(n) => Ok(EaEncoded::new(eac::DN | n)),

        EffectiveAddress::AddrReg(n) => Ok(EaEncoded::new(eac::AN | n)),

        EffectiveAddress::AddrRegInd(n) => Ok(EaEncoded::new(eac::ADR | n)),

        EffectiveAddress::AddrRegPostInc(n) => Ok(EaEncoded::new(eac::INCADR | n)),

        EffectiveAddress::AddrRegPreDec(n) => Ok(EaEncoded::new(eac::DECADR | n)),

        // ---- 16ビットディスプレースメント（32ビットはフルフォーマットへフォールバック）----
        EffectiveAddress::AddrRegDisp { an, disp } => {
            let v = eval_disp(disp)?;
            // HAS互換: displacement=0 の場合は (An) 形式に最適化
            // ただし明示的サイズ指定がある場合は抑制（-c0 での no_null_disp 対応）
            if v == 0 && disp.size.is_none() {
                return Ok(EaEncoded::new(eac::ADR | an));
            }
            if let Ok(w) = check_word(v) {
                let mut enc = EaEncoded::new(eac::DSPADR | an);
                enc.push_word(w as u16);
                Ok(enc)
            } else {
                // 32ビットディスプレースメント: 68020+ フルフォーマット拡張ワード
                // IS=1 (index suppressed), BD=11 (long), I/IS=000 (no memory indirect)
                let mut enc = EaEncoded::new(eac::IDXADR | an);
                enc.push_word(0x0170);
                enc.push_long(v as u32);
                Ok(enc)
            }
        }

        // ---- brief 拡張ワード（8ビットディスプレースメント + インデックス）----
        EffectiveAddress::AddrRegIdx { an, disp, idx } => {
            let v = eval_disp(disp)?;
            let d8 = check_byte(v)?;
            let ext = make_brief_ext(idx, d8);
            let mut enc = EaEncoded::new(eac::IDXADR | an);
            enc.push_word(ext);
            Ok(enc)
        }

        // ---- 絶対アドレス ----
        EffectiveAddress::AbsShort(rpn) => {
            let v = eval_rpn_const(rpn)?;
            // 16ビット符号付きとして表現できる値のみ許可
            let v16 = v as i16;
            if v16 as i32 != v {
                return Err(EncodeError::DisplacementOutOfRange { value: v, bits: 16 });
            }
            let mut enc = EaEncoded::new(eac::ABSW);
            enc.push_word(v16 as u16);
            Ok(enc)
        }

        EffectiveAddress::AbsLong(rpn) => {
            let v = eval_rpn_const(rpn)?;
            let mut enc = EaEncoded::new(eac::ABSL);
            enc.push_long(v as u32);
            Ok(enc)
        }

        // ---- PC 相対 ----
        EffectiveAddress::PcDisp(disp) => {
            let v = eval_disp(disp)?;
            let w = check_word(v)?;
            let mut enc = EaEncoded::new(eac::DSPPC);
            enc.push_word(w as u16);
            Ok(enc)
        }

        EffectiveAddress::PcIdx { disp, idx } => {
            let v = eval_disp(disp)?;
            let d8 = check_byte(v)?;
            let ext = make_brief_ext(idx, d8);
            let mut enc = EaEncoded::new(eac::IDXPC);
            enc.push_word(ext);
            Ok(enc)
        }

        // ---- 68020+ メモリ間接 ----
        EffectiveAddress::MemIndPost { an, bd, idx, od } => {
            let mut enc = EaEncoded::new(eac::IDXADR | an);
            encode_mem_indirect(&mut enc, idx, bd, od, true)?;
            Ok(enc)
        }
        EffectiveAddress::MemIndPre { an, bd, idx, od } => {
            let mut enc = EaEncoded::new(eac::IDXADR | an);
            encode_mem_indirect(&mut enc, idx, bd, od, false)?;
            Ok(enc)
        }
        EffectiveAddress::PcMemIndPost { bd, idx, od } => {
            let mut enc = EaEncoded::new(eac::IDXPC);
            encode_mem_indirect(&mut enc, idx, bd, od, true)?;
            Ok(enc)
        }
        EffectiveAddress::PcMemIndPre { bd, idx, od } => {
            let mut enc = EaEncoded::new(eac::IDXPC);
            encode_mem_indirect(&mut enc, idx, bd, od, false)?;
            Ok(enc)
        }

        // ---- イミディエイト ----
        EffectiveAddress::Immediate(rpn) => {
            let v = eval_rpn_const(rpn)?;
            let mut enc = EaEncoded::new(eac::IMM);
            match op_size {
                0 => enc.push_word(v as u16), // byte: 下位16bitをそのまま使用（HAS互換: -2→0xFFFE）
                1 => enc.push_word(v as u16), // word
                2 => enc.push_long(v as u32), // long
                _ => return Err(EncodeError::InvalidMode),
            }
            Ok(enc)
        }

        // CCR/SR は命令固有のエンコードが必要（encode_move/encode_orandeorimm で処理）
        EffectiveAddress::CcrReg
        | EffectiveAddress::SrReg
        | EffectiveAddress::FpReg(_)
        | EffectiveAddress::FpCtrlReg(_) => Err(EncodeError::InvalidMode),
    }
}
