/// 実効アドレスエンコード
///
/// `EffectiveAddress` → 6ビット EA フィールド + 拡張ワードバイト列

use super::{EffectiveAddress, Displacement, IndexSpec, IdxSize, Scale, DispSize, eac};
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
        EaEncoded { ea_field, ext_bytes: Vec::new() }
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
    let da_bit  = if idx.reg >= 8 { 1u16 } else { 0u16 };
    let reg_num = (idx.reg & 7) as u16;
    let sz_bit  = if idx.size == IdxSize::Long { 1u16 } else { 0u16 };
    let scale   = idx.scale as u16;
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
        EffectiveAddress::DataReg(n) =>
            Ok(EaEncoded::new(eac::DN | n)),

        EffectiveAddress::AddrReg(n) =>
            Ok(EaEncoded::new(eac::AN | n)),

        EffectiveAddress::AddrRegInd(n) =>
            Ok(EaEncoded::new(eac::ADR | n)),

        EffectiveAddress::AddrRegPostInc(n) =>
            Ok(EaEncoded::new(eac::INCADR | n)),

        EffectiveAddress::AddrRegPreDec(n) =>
            Ok(EaEncoded::new(eac::DECADR | n)),

        // ---- 16ビットディスプレースメント ----
        EffectiveAddress::AddrRegDisp { an, disp } => {
            let v = eval_disp(disp)?;
            // HAS互換: displacement=0 の場合は (An) 形式に最適化
            if v == 0 {
                return Ok(EaEncoded::new(eac::ADR | an));
            }
            let w = check_word(v)?;
            let mut enc = EaEncoded::new(eac::DSPADR | an);
            enc.push_word(w as u16);
            Ok(enc)
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

        // ---- イミディエイト ----
        EffectiveAddress::Immediate(rpn) => {
            let v = eval_rpn_const(rpn)?;
            let mut enc = EaEncoded::new(eac::IMM);
            match op_size {
                0 => enc.push_word(v as u16),  // byte: 下位16bitをそのまま使用（HAS互換: -2→0xFFFE）
                1 => enc.push_word(v as u16),  // word
                2 => enc.push_long(v as u32),  // long
                _ => return Err(EncodeError::InvalidMode),
            }
            Ok(enc)
        }

        // CCR/SR は命令固有のエンコードが必要（encode_move/encode_orandeorimm で処理）
        EffectiveAddress::CcrReg | EffectiveAddress::SrReg => {
            Err(EncodeError::InvalidMode)
        }
    }
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressing::{parse_ea, IndexSpec, IdxSize, Scale, Displacement, EffectiveAddress};
    use crate::symbol::SymbolTable;
    use crate::options::cpu;

    fn make_sym() -> SymbolTable {
        SymbolTable::new(false)
    }

    fn parse_and_encode(s: &str, op_size: u8) -> EaEncoded {
        let sym = make_sym();
        let mut pos = 0;
        let ea = parse_ea(s.as_bytes(), &mut pos, &sym, cpu::C000).expect(s);
        encode_ea(&ea, op_size).expect(&format!("encode {}", s))
    }

    // ---- EA フィールド値の確認 ----

    #[test]
    fn test_ea_field_dn() {
        assert_eq!(parse_and_encode("d0", 1).ea_field, 0b000_000);
        assert_eq!(parse_and_encode("d7", 1).ea_field, 0b000_111);
    }

    #[test]
    fn test_ea_field_an() {
        assert_eq!(parse_and_encode("a0", 1).ea_field, 0b001_000);
        assert_eq!(parse_and_encode("a3", 1).ea_field, 0b001_011);
        assert_eq!(parse_and_encode("sp", 1).ea_field, 0b001_111);
    }

    #[test]
    fn test_ea_field_adr() {
        assert_eq!(parse_and_encode("(a0)", 1).ea_field, 0b010_000);
        assert_eq!(parse_and_encode("(a5)", 1).ea_field, 0b010_101);
    }

    #[test]
    fn test_ea_field_incadr() {
        assert_eq!(parse_and_encode("(a0)+", 1).ea_field, 0b011_000);
    }

    #[test]
    fn test_ea_field_decadr() {
        assert_eq!(parse_and_encode("-(a0)", 1).ea_field, 0b100_000);
    }

    // ---- 拡張ワードの確認 ----

    #[test]
    fn test_encode_dspadr() {
        let enc = parse_and_encode("(4,a3)", 1);
        assert_eq!(enc.ea_field, eac::DSPADR | 3);
        assert_eq!(enc.ext_bytes, vec![0x00, 0x04]);  // +4
    }

    #[test]
    fn test_encode_dspadr_negative() {
        let enc = parse_and_encode("(-8,a0)", 1);
        assert_eq!(enc.ea_field, eac::DSPADR | 0);
        assert_eq!(enc.ext_bytes, vec![0xFF, 0xF8]);  // -8 as i16 = 0xFFF8
    }

    #[test]
    fn test_encode_idxadr() {
        // (2,a0,d1.w*1) brief extension word:
        // bit15=0(Dn), bits14-12=001(D1), bit11=0(.w), bits10-9=00(*1), bit8=0, bits7-0=0x02
        let enc = parse_and_encode("(2,a0,d1)", 1);
        assert_eq!(enc.ea_field, eac::IDXADR | 0);
        // D1.w*1 disp=2: 0001_0000_0000_0010 = 0x1002
        assert_eq!(enc.ext_bytes, vec![0x10, 0x02]);
    }

    #[test]
    fn test_encode_idxadr_long() {
        // (0,a3,d4.l) brief extension word:
        // D4.l*1 disp=0: bit15=0, 100(D4), 1(.l), 00(*1), 0, 0x00 = 0x4800
        let enc = parse_and_encode("(0,a3,d4.l)", 1);
        assert_eq!(enc.ea_field, eac::IDXADR | 3);
        assert_eq!(enc.ext_bytes, vec![0x48, 0x00]);
    }

    #[test]
    fn test_encode_idxadr_an_index() {
        // (0,a0,a1.w) → A1 as index: bit15=1(An), 001(A1), 0(.w), 00(*1), 0, 0x00 = 0x9000
        let enc = parse_and_encode("(0,a0,a1.w)", 1);
        assert_eq!(enc.ea_field, eac::IDXADR | 0);
        assert_eq!(enc.ext_bytes, vec![0x90, 0x00]);
    }

    #[test]
    fn test_encode_absw() {
        let enc = parse_and_encode("$1234.w", 1);
        assert_eq!(enc.ea_field, eac::ABSW);
        assert_eq!(enc.ext_bytes, vec![0x12, 0x34]);
    }

    #[test]
    fn test_encode_absl() {
        let enc = parse_and_encode("$12345678", 1);
        assert_eq!(enc.ea_field, eac::ABSL);
        assert_eq!(enc.ext_bytes, vec![0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_encode_dsppc() {
        let enc = parse_and_encode("(4,pc)", 1);
        assert_eq!(enc.ea_field, eac::DSPPC);
        assert_eq!(enc.ext_bytes, vec![0x00, 0x04]);
    }

    #[test]
    fn test_encode_idxpc() {
        // (2,pc,d0) → PC rel idx, D0.w*1, disp=2
        let enc = parse_and_encode("(2,pc,d0)", 1);
        assert_eq!(enc.ea_field, eac::IDXPC);
        // D0.w*1 disp=2: 0000_0000_0000_0010 = 0x0002
        assert_eq!(enc.ext_bytes, vec![0x00, 0x02]);
    }

    #[test]
    fn test_encode_imm_byte() {
        let enc = parse_and_encode("#$42", 0);
        assert_eq!(enc.ea_field, eac::IMM);
        assert_eq!(enc.ext_bytes, vec![0x00, 0x42]);  // byte padded to word
    }

    #[test]
    fn test_encode_imm_word() {
        let enc = parse_and_encode("#$1234", 1);
        assert_eq!(enc.ea_field, eac::IMM);
        assert_eq!(enc.ext_bytes, vec![0x12, 0x34]);
    }

    #[test]
    fn test_encode_imm_long() {
        let enc = parse_and_encode("#$12345678", 2);
        assert_eq!(enc.ea_field, eac::IMM);
        assert_eq!(enc.ext_bytes, vec![0x12, 0x34, 0x56, 0x78]);
    }

    // ---- 直接構築した EA のエンコード ----

    #[test]
    fn test_encode_manual_idx() {
        // (4,a5,d2.l*4) のエンコードを直接検証
        let ea = EffectiveAddress::AddrRegIdx {
            an: 5,
            disp: Displacement { rpn: vec![], size: None, const_val: Some(4) },
            idx: IndexSpec { reg: 2, size: IdxSize::Long, scale: Scale::S4, suppress: false },
        };
        let enc = encode_ea(&ea, 1).unwrap();
        assert_eq!(enc.ea_field, eac::IDXADR | 5);
        // D2.l*4: bit15=0(Dn), 010(D2), 1(.l), 10(*4), 0, disp=4 = 0010_1100_0000_0100 = 0x2C04
        assert_eq!(enc.ext_bytes, vec![0x2C, 0x04]);
    }

    // ---- エラーケース ----

    #[test]
    fn test_encode_disp_overflow() {
        let ea = EffectiveAddress::AddrRegDisp {
            an: 0,
            disp: Displacement { rpn: vec![], size: None, const_val: Some(0x10000) },
        };
        assert!(matches!(
            encode_ea(&ea, 1),
            Err(EncodeError::DisplacementOutOfRange { bits: 16, .. })
        ));
    }

    #[test]
    fn test_encode_brief_disp_overflow() {
        let ea = EffectiveAddress::AddrRegIdx {
            an: 0,
            disp: Displacement { rpn: vec![], size: None, const_val: Some(200) }, // > 127
            idx: IndexSpec { reg: 0, size: IdxSize::Word, scale: Scale::S1, suppress: false },
        };
        assert!(matches!(
            encode_ea(&ea, 1),
            Err(EncodeError::DisplacementOutOfRange { bits: 8, .. })
        ));
    }
}
