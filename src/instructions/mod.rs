/// 68000 命令エンコード（Phase 5）
///
/// `encode_insn(base_opcode, handler, size, operands)` → `Vec<u8>`
///
/// 入力はすでに解析済みの EffectiveAddress。
/// シンボル参照を含む EA は `InsnError::DeferToLinker` を返す。

use crate::addressing::{
    EffectiveAddress,
    encode::{encode_ea, EncodeError, EaEncoded},
};
use crate::expr::Rpn;
use crate::expr::eval_rpn;
use crate::symbol::types::{InsnHandler, SizeCode};

// ----------------------------------------------------------------
// 公開型
// ----------------------------------------------------------------

/// 命令エンコードエラー
#[derive(Debug, Clone, PartialEq)]
pub enum InsnError {
    /// シンボル参照あり（Pass 7 で解決）
    DeferToLinker,
    /// 不正なサイズ指定
    InvalidSize,
    /// 不正なオペランド型
    InvalidOperand,
    /// 不正なアドレッシングモード
    InvalidAddressingMode,
    /// オペランド数が合わない
    OperandCount,
    /// 値が範囲外
    OutOfRange { value: i32, min: i32, max: i32 },
}

// ----------------------------------------------------------------
// 内部ユーティリティ
// ----------------------------------------------------------------

/// big-endian でワードを積む
fn push_word(bytes: &mut Vec<u8>, w: u16) {
    bytes.push((w >> 8) as u8);
    bytes.push(w as u8);
}

/// big-endian でロングワードを積む
fn push_long(bytes: &mut Vec<u8>, l: u32) {
    bytes.push((l >> 24) as u8);
    bytes.push((l >> 16) as u8);
    bytes.push((l >>  8) as u8);
    bytes.push(l as u8);
}

/// サイズコード → op_size (encode_ea の第2引数: 0=byte, 1=word, 2=long)
fn size_to_op_size(size: SizeCode) -> Result<u8, InsnError> {
    match size {
        SizeCode::Byte => Ok(0),
        SizeCode::Word => Ok(1),
        SizeCode::Long => Ok(2),
        _ => Err(InsnError::InvalidSize),
    }
}

/// サイズコード → bits 7-6 (00=byte, 01=word, 10=long)
fn size_field(size: SizeCode) -> Result<u16, InsnError> {
    match size {
        SizeCode::Byte => Ok(0x00),
        SizeCode::Word => Ok(0x40),
        SizeCode::Long => Ok(0x80),
        _ => Err(InsnError::InvalidSize),
    }
}

/// RPN を定数評価する（シンボル参照があれば None）
fn eval_const(rpn: &Rpn) -> Option<i32> {
    if rpn.is_empty() {
        return Some(0);
    }
    match eval_rpn(rpn, 0, 0, 0, &|_| None) {
        Ok(v) if v.section == 0 => Some(v.value),
        _ => None,
    }
}

/// EncodeError → InsnError 変換
fn map_enc_err(e: EncodeError) -> InsnError {
    match e {
        EncodeError::DeferToLinker => InsnError::DeferToLinker,
        EncodeError::InvalidMode   => InsnError::InvalidAddressingMode,
        EncodeError::DisplacementOutOfRange { value, bits } => {
            let half = 1i32 << (bits - 1);
            InsnError::OutOfRange { value, min: -half, max: half - 1 }
        }
    }
}

/// EA をエンコードする（失敗時は InsnError に変換）
fn enc(ea: &EffectiveAddress, op_size: u8) -> Result<EaEncoded, InsnError> {
    encode_ea(ea, op_size).map_err(map_enc_err)
}

/// DataReg なら番号を返す
fn data_reg(ea: &EffectiveAddress) -> Option<u8> {
    if let EffectiveAddress::DataReg(n) = ea { Some(*n) } else { None }
}

/// AddrReg なら番号を返す
fn addr_reg(ea: &EffectiveAddress) -> Option<u8> {
    if let EffectiveAddress::AddrReg(n) = ea { Some(*n) } else { None }
}

/// Immediate の RPN を返す
fn imm_rpn(ea: &EffectiveAddress) -> Option<&Rpn> {
    if let EffectiveAddress::Immediate(rpn) = ea { Some(rpn) } else { None }
}

// ----------------------------------------------------------------
// ハンドラ実装
// ----------------------------------------------------------------

/// Bcc / no-operand 命令（NOP/RTS/RTE 等 + 分岐）
///
/// no-operand: オペランドなし → base_opcode をそのまま出力
/// 分岐: ターゲットがある → DeferToLinker（Phase 7 で解決）
fn encode_bcc(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.is_empty() {
        let mut v = Vec::with_capacity(2);
        push_word(&mut v, base);
        return Ok(v);
    }
    // 分岐ターゲットはシンボル参照（PC相対）が必要 → Phase 7 で処理
    Err(InsnError::DeferToLinker)
}

/// JMP/JSR (EA → ctrl モードのみ)
fn encode_jmpjsr(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// MOVE <src>, <dst>
///
/// MOVE のサイズエンコードは特殊:
/// - .b → 0x1000 (bits 15-12 = 0001)
/// - .w → 0x3000 (bits 15-12 = 0011)
/// - .l → 0x2000 (bits 15-12 = 0010)
///
/// 宛先 EA: bits 11-9=reg, bits 8-6=mode（通常と逆）
fn encode_move(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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
    let dst_reg  = ( dst_enc.ea_field       & 7) as u16;
    let dst_field = (dst_reg << 9) | (dst_mode << 6);

    let word = size_top | dst_field | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    v.extend_from_slice(&dst_enc.ext_bytes);
    Ok(v)
}

/// MOVEA <src>, An
///
/// base_opcode = 0x2040 (Long form)
/// Word → bit 12 を追加 → 0x3040
fn encode_movea(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let size_bit: u16 = match size {
        SizeCode::Word => 0x1000,  // 0x3040 pattern
        SizeCode::Long => 0x0000,  // 0x2040 pattern
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

/// MOVEQ #imm, Dn
///
/// base_opcode = 0x7000
/// 8ビット即値（-128～255）を bits 7-0 に入れ、Dn を bits 11-9 に
fn encode_moveq(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if val < -128 || val > 255 {
        return Err(InsnError::OutOfRange { value: val, min: -128, max: 255 });
    }
    let word = 0x7000u16 | ((dn as u16) << 9) | (val as u8 as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// MOVEM <reglist>, <ea>  /  MOVEM <ea>, <reglist>
///
/// base_opcode = 0x4880
/// bit 10 = 方向 (0=reg→mem, 1=mem→reg)
/// bit 6  = サイズ (0=word, 1=long)
/// bits 5-0 = EA
/// 次ワード = 16ビットレジスタマスク
///
/// オペランド:
///   operands[0] = Immediate(mask_rpn) = レジスタリスト（Phase 5では定数のみ）
///   operands[1] = EA（メモリ）
///   または逆順（mem→reg）
fn encode_movem(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0000,
        SizeCode::Long => 0x0040,
        _ => return Err(InsnError::InvalidSize),
    };
    // operands[0] が Immediate → reg→mem 方向
    // operands[1] が Immediate → mem→reg 方向
    let (dir_bit, reglist_rpn, ea) = if let Some(rpn) = imm_rpn(&operands[0]) {
        (0x0000u16, rpn, &operands[1])
    } else if let Some(rpn) = imm_rpn(&operands[1]) {
        (0x0400u16, rpn, &operands[0])
    } else {
        return Err(InsnError::InvalidOperand);
    };
    let mask = eval_const(reglist_rpn).ok_or(InsnError::DeferToLinker)?;
    // -(An) の場合、レジスタマスクを反転する（D7→bit0, A7→bit8）
    let (mask_word, is_predec) = if matches!(ea, EffectiveAddress::AddrRegPreDec(_)) {
        (reverse_bits16(mask as u16), true)
    } else {
        (mask as u16, false)
    };
    let _ = is_predec;  // 使用済みとしてマーク
    let ea_enc = enc(ea, 1)?;
    let word = 0x4880u16 | dir_bit | sz_bit | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, mask_word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// 16ビットのビット反転（MOVEM -(An) 用）
fn reverse_bits16(x: u16) -> u16 {
    x.reverse_bits()
}

/// MOVEP Dn, (d,An)  /  MOVEP (d,An), Dn
///
/// base_opcode = 0x0108
/// bit 7 = 方向 (0=mem→Dn, 1=Dn→mem)
/// bit 6 = サイズ (0=word, 1=long)
/// bits 11-9 = Dn, bits 2-0 = An
/// 次ワード = 16ビットディスプレースメント
fn encode_movep(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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
        return Err(InsnError::OutOfRange { value: d_val, min: -32768, max: 32767 });
    }
    let word = 0x0108u16 | sz_bit | dir_bit | ((dn as u16) << 9) | (an as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, d_val as u16);
    Ok(v)
}

/// LEA <ea>, An
///
/// base_opcode = 0x41C0
/// bits 11-9 = An, bits 5-0 = EA
fn encode_lea(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// PEA/JSR/JMP 共通ハンドラ（ctrl EA のみ）
///
/// base_opcode に EA フィールドを OR する
fn encode_peajsrjmp(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// ADD/SUB 汎用ハンドラ（SubAdd）
///
/// base_opcode: ADD=0xD000, SUB=0x9000
///
/// 形式:
///   Dn, <ea> → dir=1 (bit 8 set)
///   <ea>, Dn → dir=0
///   #imm, <ea> → ADDI/SUBI encoding (0x0600/0x0400)
///   #imm, An / <ea>, An → ADDA/SUBA encoding
///   Dn, An / An, Dn / ... → handled by SbAdCpA
fn encode_subadd(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;

    match (&operands[0], &operands[1]) {
        // #imm, An → ADDA/SUBA
        (EffectiveAddress::Immediate(_), EffectiveAddress::AddrReg(_)) => {
            let adda_base = if base & 0x4000 != 0 { 0xD0C0u16 } else { 0x90C0u16 };
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
            let imm_base = if base & 0x4000 != 0 { 0x0600u16 } else { 0x0400u16 };
            encode_subaddi(imm_base, size, &[operands[0].clone(), dst.clone()])
        }
        // <ea>, An → ADDA/SUBA
        (_, EffectiveAddress::AddrReg(_)) => {
            let adda_base = if base & 0x4000 != 0 { 0xD0C0u16 } else { 0x90C0u16 };
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

/// ADDQ/SUBQ #imm(1-8), <ea>
///
/// base_opcode: ADDQ=0x5000, SUBQ=0x5100
/// bits 11-9 = quick value (0 → 8)
/// bits 7-6 = size, bits 5-0 = EA
fn encode_subaddq(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if val < 1 || val > 8 {
        return Err(InsnError::OutOfRange { value: val, min: 1, max: 8 });
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

/// ADDI/SUBI #imm, <ea>
///
/// base_opcode: ADDI=0x0600, SUBI=0x0400
/// bits 7-6 = size, bits 5-0 = EA
/// 次ワード(s): immediate data
fn encode_subaddi(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// ADDA/SUBA/CMPA <ea>, An
///
/// base_opcode: ADDA=0xD0C0, SUBA=0x90C0, CMPA=0xB0C0
/// Word: bit 8 clear (bits 8-6 = 011)
/// Long: bit 8 set   (bits 8-6 = 111)
/// bits 11-9 = An, bits 5-0 = EA
fn encode_sbadcpa(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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
    // base has bits 8-6 = 011 (word). For long, OR 0x0100 to get 111.
    let word = base | size_bit | ((an as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

/// ADDX/SUBX Ry, Rx  /  -(Ay), -(Ax)
///
/// base_opcode: ADDX=0xD100, SUBX=0x9100
/// bit 3 = mode (0=Dn/Dn, 1=-(An)/-(An))
/// bits 7-6 = size, bits 11-9 = Rx, bits 2-0 = Ry
fn encode_subaddx(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let (mode_bit, ry, rx) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(y), EffectiveAddress::DataReg(x)) =>
            (0x0000u16, *y, *x),
        (EffectiveAddress::AddrRegPreDec(y), EffectiveAddress::AddrRegPreDec(x)) =>
            (0x0008u16, *y, *x),
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = base | sz | ((rx as u16) << 9) | mode_bit | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// CMP <ea>, Dn
///
/// base_opcode = 0xB000
/// bits 7-6 = size, bits 11-9 = Dn, bits 5-0 = EA
fn encode_cmp(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// CMPI #imm, <ea>
///
/// base_opcode = 0x0C00
fn encode_cmpi(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_subaddi(base, size, operands)
}

/// CMPA <ea>, An  (CmpA handler)
fn encode_cmpa(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_sbadcpa(base, size, operands)
}

/// CMPM (Ay)+, (Ax)+
///
/// base_opcode = 0xB108
/// bits 7-6 = size, bits 11-9 = Ax, bits 2-0 = Ay
fn encode_cmpm(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// NEG/NEGX/NOT/NBCDなど単項 EA 命令（NegNot ハンドラ）
///
/// base_opcode に size + EA をマージ
fn encode_negnot(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// CLR <ea>（Clr ハンドラ）
fn encode_clr(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_negnot(base, size, operands)
}

/// TST <ea>（Tst ハンドラ）
fn encode_tst(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_negnot(base, size, operands)
}

/// EXT.W Dn / EXT.L Dn
///
/// base_opcode = 0x4880
/// EXT.W: bits 8-6 = 010 → 0x4880 (bit 7=0)
/// EXT.L: bits 8-6 = 011 → 0x48C0 (bit 7=1)
/// bits 2-0 = Dn
fn encode_ext(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0000,   // 0x4880
        SizeCode::Long => 0x0040,   // 0x48C0
        _ => return Err(InsnError::InvalidSize),
    };
    let word = 0x4880u16 | sz_bit | (dn as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// SWAP Dn
///
/// base_opcode = 0x4840
/// bits 2-0 = Dn
fn encode_swap(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let word = 0x4840u16 | (dn as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// EXG Rx, Ry
///
/// base_opcode = 0xC100
/// 3 variants:
///   Dn, Dn → bits 7-3 = 01000 (mode 0x08)
///   An, An → bits 7-3 = 01001 (mode 0x09)
///   Dn, An → bits 7-3 = 10001 (mode 0x11)
/// bits 11-9 = Rx, bits 2-0 = Ry
fn encode_exg(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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
    // base = 0xC100 has bits 7-3 unset; OR the mode
    let word = 0xC100u16 | ((rx as u16) << 9) | (mode << 3) | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// MULU/MULS/DIVU/DIVS <ea>, Dn (68000 word form only)
///
/// base_opcode: MULU=0xC0C0, MULS=0xC1C0, DIVU=0x80C0, DIVS=0x81C0
fn encode_divmul(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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

/// CHK <ea>, Dn
///
/// base_opcode = 0x4100
/// bits 11-9 = Dn, bits 5-0 = EA
/// Word form (68000): bit 7 = 1 (bits 8-6 = 110)
fn encode_chk(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let sz_bit: u16 = match size {
        SizeCode::Word => 0x0080,   // bits 8-6 = 110 → base 0x4100 | 0x0080 = 0x4180
        SizeCode::Long => 0x0000,   // bits 8-6 = 100 → base 0x4100 (68020+)
        _ => return Err(InsnError::InvalidSize),
    };
    let src_enc = enc(&operands[0], 1)?;
    let word = 0x4100u16 | sz_bit | ((dn as u16) << 9) | (src_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&src_enc.ext_bytes);
    Ok(v)
}

/// ABCD/SBCD Ry, Rx  /  -(Ay), -(Ax)
///
/// base_opcode: ABCD=0xC100, SBCD=0x8100
/// bit 3 = mode (0=Dn/Dn, 1=-(An)/-(An))
/// bits 11-9 = Rx, bits 2-0 = Ry
fn encode_sabcd(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let (mode_bit, ry, rx) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(y), EffectiveAddress::DataReg(x)) => (0x0000u16, *y, *x),
        (EffectiveAddress::AddrRegPreDec(y), EffectiveAddress::AddrRegPreDec(x)) => (0x0008u16, *y, *x),
        _ => return Err(InsnError::InvalidOperand),
    };
    let word = base | ((rx as u16) << 9) | mode_bit | (ry as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// AND/OR <ea>, Dn  /  Dn, <ea>  (OrAnd ハンドラ)
///
/// base_opcode: AND=0xC000, OR=0x8000
/// Dn, <ea>: dir=1 (bit 8)
/// <ea>, Dn: dir=0
/// #imm: → ANDI/ORI
fn encode_orand(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
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
            let imm_base = if base & 0x4000 != 0 { 0x0200u16 } else { 0x0000u16 };
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

/// EOR Dn, <ea>（Eor ハンドラ）
///
/// base_opcode = 0xB100
fn encode_eor(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;

    match (&operands[0], &operands[1]) {
        // #imm, <ea> → EORI
        (EffectiveAddress::Immediate(_), _) => {
            encode_orandeorimm(0x0A00, size, operands)
        }
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

/// ORI/ANDI/EORI #imm, <ea>（OrAndEorI ハンドラ）
///
/// base_opcode: ORI=0x0000, ANDI=0x0200, EORI=0x0A00
/// 特殊: #imm, CCR / #imm, SR → 固定オペコード
fn encode_orandeorimm(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;

    // 特殊ケース: #imm, CCR / #imm, SR
    // ORI/ANDI/EORI の CCR/SR 向け固定オペコード:
    //   base=0x0000(ORI):  CCR→0x003C, SR→0x007C
    //   base=0x0200(ANDI): CCR→0x023C, SR→0x027C
    //   base=0x0A00(EORI): CCR→0x0A3C, SR→0x0A7C
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

/// NOT <ea>（NegNot ハンドラと共用）
fn encode_not(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_negnot(base, size, operands)
}

/// BTST/BSET/BCLR/BCHG #imm/<Dn>, <ea>
///
/// 2 forms:
///   #imm, <ea>: static (0x0800 + sub-op + EA)
///   Dn, <ea>:   dynamic (0x0100 + sub-op + Dn<<9 + EA)
///
/// base_opcode: BTST=0x0000(+0x0800), BSET=0x00C0, BCLR=0x0080, BCHG=0x0040
/// For dynamic (Dn): base | (Dn<<9) | EA; opcode field adds 0x0100
/// For static (#imm): base | 0x0800 | EA; next byte = bit number
fn encode_bchclst(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    match (&operands[0], &operands[1]) {
        (EffectiveAddress::Immediate(rpn), dst) => {
            // Static bit: base | 0x0800 | EA
            let bit_num = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            if bit_num < 0 || bit_num > 31 {
                return Err(InsnError::OutOfRange { value: bit_num, min: 0, max: 31 });
            }
            let dst_enc = enc(dst, 0)?;
            let word = base | 0x0800 | (dst_enc.ea_field as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_word(&mut v, bit_num as u16);  // byte in upper word, padded
            v.extend_from_slice(&dst_enc.ext_bytes);
            Ok(v)
        }
        (EffectiveAddress::DataReg(dn), dst) => {
            // Dynamic bit: base | 0x0100 | (Dn<<9) | EA
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

/// BTST (Btst ハンドラ)
fn encode_btst(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_bchclst(0x0000, operands)
}

/// ASL/ASR/LSL/LSR/ROL/ROR/ROXL/ROXR（SftRot ハンドラ）
///
/// 3 forms:
///   #imm, Dn: count(1-8) in bits 11-9, Dn in bits 2-0, bits 4-3=type, bit5=0
///   Dn, Dn:   Dn_count in 11-9, Dn_dest in 2-0, bits 4-3=type, bit5=1
///   <ea>:     word only, EA in bits 5-0, bits 11-8 are fixed for shift type
///
/// base_opcode: ASR=0xE000, ASL=0xE100, LSR=0xE008, LSL=0xE108,
///              ROR=0xE018, ROL=0xE118, ROXR=0xE010, ROXL=0xE110
///
/// Shift type bits (from base): bits 4-3 of base_opcode & 0x0018
///   ASR/ASL: 00, LSR/LSL: 01, ROXR/ROXL: 10, ROR/ROL: 11 (after mask)
fn encode_sftrot(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    let sz = size_field(size)?;
    // Memory shift form: one EA operand, word size only
    if operands.len() == 1 {
        if size != SizeCode::Word {
            return Err(InsnError::InvalidSize);
        }
        let ea_enc = enc(&operands[0], 1)?;
        // Memory shift opcode: `1110 TTT D 11 ea`
        // TTT = type bits (from base bits 4-3): AS=000, LS=001, ROX=010, RO=011
        // D = direction (from base bit 8): 0=right, 1=left
        // bits 7-6 = 11 (memory form marker)
        let type_bits = ((base & 0x0018) >> 3) as u16;  // LS=1, AS=0, ROX=2, RO=3
        let dir_bit   = ((base & 0x0100) >> 8) as u16;  // 1=left, 0=right
        let word = 0xE000u16 | (type_bits << 9) | (dir_bit << 8) | 0x00C0 | (ea_enc.ea_field as u16);
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
        // #imm, Dn: count in bits 11-9 (1-8, 8→0)
        EffectiveAddress::Immediate(rpn) => {
            let count = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
            if count < 1 || count > 8 {
                return Err(InsnError::OutOfRange { value: count, min: 1, max: 8 });
            }
            let cnt = if count == 8 { 0u16 } else { count as u16 };
            // Register shift: bit 5=0
            let word = (base & 0xFFF8) | sz | (cnt << 9) | (dest_dn as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            Ok(v)
        }
        // Dn, Dn: bit 5=1
        EffectiveAddress::DataReg(src_dn) => {
            let word = (base & 0xFFF8) | sz | 0x0020 | ((*src_dn as u16) << 9) | (dest_dn as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

/// ASL 専用ハンドラ（SftRot と同じロジック）
fn encode_asl(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    encode_sftrot(base, size, operands)
}

/// DBcc Dn, <label>
///
/// base_opcode: DBRA=0x51C8, DBcc=0x52C8 etc
/// bits 2-0 = Dn
/// 次ワード = 16ビット相対ディスプレースメント → DeferToLinker
fn encode_dbcc(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    // ブランチターゲットは PC 相対 → DeferToLinker
    let _ = dn;
    Err(InsnError::DeferToLinker)
}

/// Scc/NBCD/TAS <ea>（Scc ハンドラ）
///
/// base_opcode: ST=0x50C0 etc., NBCD=0x4800, TAS=0x4AC0
/// bits 5-0 = EA
fn encode_scc(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let ea_enc = enc(&operands[0], 0)?;
    let word = base | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// LINK An, #disp
///
/// base_opcode = 0x4E50
/// bits 2-0 = An
/// 次ワード = 16ビットディスプレースメント
fn encode_link(size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let rpn = imm_rpn(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let disp = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    match size {
        SizeCode::Word | SizeCode::None => {
            if disp < i16::MIN as i32 || disp > i16::MAX as i32 {
                return Err(InsnError::OutOfRange { value: disp, min: -32768, max: 32767 });
            }
            let word = 0x4E50u16 | (an as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_word(&mut v, disp as u16);
            Ok(v)
        }
        SizeCode::Long => {
            // LINK.L (68020+): 0x4808
            let word = 0x4808u16 | (an as u16);
            let mut v = Vec::new();
            push_word(&mut v, word);
            push_long(&mut v, disp as u32);
            Ok(v)
        }
        _ => Err(InsnError::InvalidSize),
    }
}

/// UNLK An
///
/// base_opcode = 0x4E58
/// bits 2-0 = An
fn encode_unlk(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let an = addr_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let word = 0x4E58u16 | (an as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// TRAP #n
///
/// base_opcode = 0x4E40
/// bits 3-0 = trap vector (0-15)
fn encode_trap(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let vec_num = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    if vec_num < 0 || vec_num > 15 {
        return Err(InsnError::OutOfRange { value: vec_num, min: 0, max: 15 });
    }
    let word = 0x4E40u16 | (vec_num as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    Ok(v)
}

/// STOP #imm  /  RTD #imm（StopRtd ハンドラ）
///
/// base_opcode: STOP=0x4E72, RTD=0x4E74
/// 次ワード = 即値（STOP→SR値、RTD→ディスプレースメント）
fn encode_stoprtd(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
    let mut v = Vec::new();
    push_word(&mut v, base);
    push_word(&mut v, val as u16);
    Ok(v)
}

/// DEC/INC（HAS 独自拡張: SUBQ/ADDQ #1, <ea> のエイリアス）
///
/// base_opcode: DEC=0x5300(subq #1), INC=0x5200(addq #1)
fn encode_decinc(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 {
        return Err(InsnError::OperandCount);
    }
    // subq/addq #1, <ea> として組み立て
    let sz = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[0], op_size)?;
    // Quick value = 1 → bits 11-9 = 001
    let word = base | (1u16 << 9) | sz | (ea_enc.ea_field as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

// ----------------------------------------------------------------
// Phase 9: 68010+/68020+ 拡張命令エンコーダ
// ----------------------------------------------------------------

/// EXTB.L Dn – バイト→ロング符号拡張（68020+）
/// opcode: 0x49C0 | reg
fn encode_extb(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 { return Err(InsnError::OperandCount); }
    let r = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let mut v = Vec::new();
    push_word(&mut v, 0x49C0 | (r as u16));
    Ok(v)
}

/// BKPT #n – ブレークポイント（68010+）
/// opcode: 0x4848 | (n & 7)
fn encode_bkpt(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 { return Err(InsnError::OperandCount); }
    let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let n = eval_const(rpn).ok_or(InsnError::DeferToLinker)? & 7;
    let mut v = Vec::new();
    push_word(&mut v, 0x4848 | (n as u16));
    Ok(v)
}

/// TRAPcc / TRAPcc.W #imm / TRAPcc.L #imm（68020+）
/// base: 条件別 0x50F8〜0x5FF8
/// no operand → |= 0x0004, .W → |= 0x02 + word ext, .L → |= 0x03 + long ext
fn encode_trapcc(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    let mut v = Vec::new();
    if operands.is_empty() {
        push_word(&mut v, base | 0x0004);
    } else {
        let rpn = imm_rpn(&operands[0]).ok_or(InsnError::InvalidOperand)?;
        let val = eval_const(rpn).ok_or(InsnError::DeferToLinker)?;
        match size {
            SizeCode::Word | SizeCode::None => {
                push_word(&mut v, base | 0x0002);
                push_word(&mut v, val as u16);
            }
            SizeCode::Long => {
                push_word(&mut v, base | 0x0003);
                push_long(&mut v, val as u32);
            }
            _ => return Err(InsnError::InvalidSize),
        }
    }
    Ok(v)
}

/// ビットフィールド指定子 {offset:width} を拡張ワードとして解析する
///
/// 返値: (bitfield_ext_word, text_after_parsed_len)
/// 実際の実装では parse_ea の後に残りテキストを再解析する必要があるが、
/// ここでは EffectiveAddress の AbsLong を {offset:width} として流用する。
/// Phase 9 では EA を通常通り解析した後、bitfield 引数を補助で読む必要がある。
/// 簡略化: 最後の2オペランドが offset(Immediate) と width(Immediate) であると仮定。
fn parse_bitfield_ext(ops: &[EffectiveAddress]) -> Option<(u16, usize)> {
    // ops: [ea, offset, width] または [ea, offset, width, Dn(dest)]
    // offset: Immediate(RPN) or DataReg
    // width:  Immediate(RPN) or DataReg
    // 引数の個数確認
    if ops.len() < 3 { return None; }
    let offset_ext = match &ops[1] {
        EffectiveAddress::DataReg(r) => {
            // レジスタ offset: bit 11 set
            (0x0800u16 | ((*r & 7) as u16))
        }
        EffectiveAddress::Immediate(rpn) => {
            let v = eval_const(rpn)? as u16 & 0x1F;
            v << 6
        }
        _ => return None,
    };
    let width_ext = match &ops[2] {
        EffectiveAddress::DataReg(r) => {
            0x0020u16 | ((*r & 7) as u16)
        }
        EffectiveAddress::Immediate(rpn) => {
            let v = eval_const(rpn)? as u16 & 0x1F;
            v // 0 means 32
        }
        _ => return None,
    };
    Some((offset_ext | width_ext, 3))
}

/// BFTST/BFCHG/BFCLR/BFSET <ea>{offset:width}（68020+）
fn encode_bitfield_1ea(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() < 3 { return Err(InsnError::OperandCount); }
    let ea_enc = enc(&operands[0], 1u8)?;
    let (bf_ext, _) = parse_bitfield_ext(operands).ok_or(InsnError::InvalidOperand)?;
    let mut v = Vec::new();
    push_word(&mut v, base | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// BFEXTU/BFEXTS/BFFFO <ea>{offset:width},Dn（68020+）
fn encode_bitfield_extract(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() < 4 { return Err(InsnError::OperandCount); }
    let ea_enc = enc(&operands[0], 1u8)?;
    let (mut bf_ext, n) = parse_bitfield_ext(operands).ok_or(InsnError::InvalidOperand)?;
    let dn = data_reg(&operands[n]).ok_or(InsnError::InvalidOperand)?;
    bf_ext |= (dn as u16) << 12;
    let mut v = Vec::new();
    push_word(&mut v, base | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// BFINS Dn,<ea>{offset:width}（68020+）
fn encode_bfins(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    // ops: [src_Dn, ea, offset, width]
    if operands.len() < 4 { return Err(InsnError::OperandCount); }
    let dn = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let ea_enc = enc(&operands[1], 1u8)?;
    let (mut bf_ext, _) = parse_bitfield_ext(&operands[1..]).ok_or(InsnError::InvalidOperand)?;
    bf_ext |= (dn as u16) << 12;
    let mut v = Vec::new();
    push_word(&mut v, 0xEFC0 | (ea_enc.ea_field as u16));
    push_word(&mut v, bf_ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// MOVES.sz Rn,<ea> / MOVES.sz <ea>,Rn（68010+）
/// opcode: 0x0E00 | size | ea
/// extension: Rn direction (bit 11)
fn encode_moves(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 { return Err(InsnError::OperandCount); }
    let sz_bits = size_field(size)?;
    let op_size = size_to_op_size(size)?;
    // 方向を判定: Rn が最初なら Rn→EA, EA が最初なら EA→Rn
    let (rn_idx, ea_idx, dir_bit) = if is_reg(&operands[0]) {
        (0, 1, 0x0800u16) // Rn,<ea>: dir=1
    } else {
        (1, 0, 0u16) // <ea>,Rn: dir=0
    };
    let ea_enc = enc(&operands[ea_idx], op_size)?;
    let rn = reg_code(&operands[rn_idx]).ok_or(InsnError::InvalidOperand)?;
    let ext_word = dir_bit | ((rn as u16) << 12);
    let mut v = Vec::new();
    push_word(&mut v, base | sz_bits | (ea_enc.ea_field as u16));
    push_word(&mut v, ext_word);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// MOVEC Rn,CReg / MOVEC CReg,Rn（68010+）
/// opcode: 0x4E7A (CReg→Rn) or 0x4E7B (Rn→CReg)
/// extension word: Rn (bits 15-12) | CReg code (bits 11-0)
fn encode_movec(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 { return Err(InsnError::OperandCount); }
    // 制御レジスタコードの取得（AbsLong またはシンボルとして渡される）
    // HAS では制御レジスタ名がシンボルとして登録されているが、
    // 簡略化: Immediate として扱い、値を直接使用
    let (reg_op, creg_op, dir) = if is_reg(&operands[0]) {
        // Rn,CReg → 0x4E7B
        (&operands[0], &operands[1], base | 0x0001)
    } else {
        // CReg,Rn → 0x4E7A
        (&operands[1], &operands[0], base)
    };
    let rn = reg_code(reg_op).ok_or(InsnError::InvalidOperand)?;
    let creg = match creg_op {
        EffectiveAddress::Immediate(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 0x0FFF
        }
        EffectiveAddress::AbsLong(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 0x0FFF
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let ext = ((rn as u16) << 12) | creg;
    let mut v = Vec::new();
    push_word(&mut v, dir);
    push_word(&mut v, ext);
    Ok(v)
}

/// PACK/UNPK Dn,Dn,#adj / -(An),-(An),#adj（68020+）
fn encode_packunpk(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 3 { return Err(InsnError::OperandCount); }
    let adj_rpn = imm_rpn(&operands[2]).ok_or(InsnError::InvalidOperand)?;
    let adj = eval_const(adj_rpn).ok_or(InsnError::DeferToLinker)? as u16;
    let (word, mode_bit) = match (&operands[0], &operands[1]) {
        (EffectiveAddress::DataReg(s), EffectiveAddress::DataReg(d)) => {
            let w = base | ((*d as u16) << 9) | (*s as u16);
            (w, false)
        }
        (EffectiveAddress::AddrRegPreDec(s), EffectiveAddress::AddrRegPreDec(d)) => {
            let w = base | 0x0008 | ((*d as u16) << 9) | (*s as u16);
            (w, true)
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let _ = mode_bit;
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, adj);
    Ok(v)
}

/// CAS Dc,Du,<ea>（68020+）
/// opcode: 0x08C0 | size | ea
/// extension: Du (bits 8-6) | Dc (bits 2-0)
fn encode_cas(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 3 { return Err(InsnError::OperandCount); }
    let dc = data_reg(&operands[0]).ok_or(InsnError::InvalidOperand)?;
    let du = data_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[2], op_size)?;
    // size bits 10-9: byte=01, word=10, long=11
    let sz_bits = match size {
        SizeCode::Byte => 0x0200u16,
        SizeCode::Word => 0x0400u16,
        SizeCode::Long => 0x0600u16,
        _ => return Err(InsnError::InvalidSize),
    };
    let word = 0x08C0 | sz_bits | (ea_enc.ea_field as u16);
    let ext = ((du as u16) << 6) | (dc as u16);
    let mut v = Vec::new();
    push_word(&mut v, word);
    push_word(&mut v, ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// CMP2/CHK2 <ea>,Rn（68020+）
/// opcode: base | size | ea
/// extension: Rn (bits 15-12) | CHK2 flag (bit 11)
fn encode_cmpchk2(base: u16, size: SizeCode, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 { return Err(InsnError::OperandCount); }
    let op_size = size_to_op_size(size)?;
    let ea_enc = enc(&operands[0], op_size)?;
    let rn = reg_code(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    // size bits 10-9: byte=00, word=01, long=10
    let sz_bits = match size {
        SizeCode::Byte => 0x0000u16,
        SizeCode::Word => 0x0200u16,
        SizeCode::Long => 0x0400u16,
        _ => return Err(InsnError::InvalidSize),
    };
    // CHK2 uses bit 11 in extension word
    let is_chk2 = base == 0x0800;
    let ext = ((rn as u16) << 12) | if is_chk2 { 0x0800u16 } else { 0u16 };
    let mut v = Vec::new();
    push_word(&mut v, 0x00C0 | sz_bits | (ea_enc.ea_field as u16));
    push_word(&mut v, ext);
    v.extend_from_slice(&ea_enc.ext_bytes);
    Ok(v)
}

/// MOVE16 (Ax)+,(Ay)+ / (Ax)+,abs / abs,(Ax)+ / (Ax),abs / abs,(Ax) (68040+)
fn encode_move16(operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 { return Err(InsnError::OperandCount); }
    match (&operands[0], &operands[1]) {
        (EffectiveAddress::AddrRegPostInc(ax), EffectiveAddress::AddrRegPostInc(ay)) => {
            // MOVE16 (Ax)+,(Ay)+ → 0xF620 | ax, ext_word: ay<<12|0x8000
            let mut v = Vec::new();
            push_word(&mut v, 0xF620 | (*ax as u16));
            push_word(&mut v, 0x8000 | ((*ay as u16) << 12));
            Ok(v)
        }
        (EffectiveAddress::AddrRegPostInc(ax), EffectiveAddress::AbsLong(rpn)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF610 | (*ax as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        (EffectiveAddress::AbsLong(rpn), EffectiveAddress::AddrRegPostInc(ay)) => {
            let addr = eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u32;
            let mut v = Vec::new();
            push_word(&mut v, 0xF618 | (*ay as u16));
            push_long(&mut v, addr);
            Ok(v)
        }
        _ => Err(InsnError::InvalidOperand),
    }
}

/// CINVL/CINVP/CPUSHL/CPUSHP cache_set,(An)（68040+）
/// キャッシュセット: BC=3, IC=2, DC=1 → bits 7-6
fn encode_cinvpush_lp(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 2 { return Err(InsnError::OperandCount); }
    // キャッシュタイプは Immediate として解析
    let cache = match &operands[0] {
        EffectiveAddress::Immediate(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 3
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let an = addr_reg(&operands[1]).ok_or(InsnError::InvalidOperand)?;
    let mut v = Vec::new();
    push_word(&mut v, base | (cache << 6) | (an as u16));
    Ok(v)
}

/// CINVA/CPUSHA cache_set（68040+）
fn encode_cinvpush_a(base: u16, operands: &[EffectiveAddress]) -> Result<Vec<u8>, InsnError> {
    if operands.len() != 1 { return Err(InsnError::OperandCount); }
    let cache = match &operands[0] {
        EffectiveAddress::Immediate(rpn) => {
            eval_const(rpn).ok_or(InsnError::DeferToLinker)? as u16 & 3
        }
        _ => return Err(InsnError::InvalidOperand),
    };
    let mut v = Vec::new();
    push_word(&mut v, base | (cache << 6));
    Ok(v)
}

/// レジスタコードを取得する（Dn=0-7, An=8-15）
fn reg_code(ea: &EffectiveAddress) -> Option<u8> {
    match ea {
        EffectiveAddress::DataReg(r) => Some(*r),
        EffectiveAddress::AddrReg(r) => Some(8 + *r),
        _ => None,
    }
}

/// 任意のレジスタかどうか
fn is_reg(ea: &EffectiveAddress) -> bool {
    matches!(ea, EffectiveAddress::DataReg(_) | EffectiveAddress::AddrReg(_))
}

// ----------------------------------------------------------------
// メインディスパッチ
// ----------------------------------------------------------------

/// 命令をエンコードする
///
/// * `base_opcode` - シンボルテーブルの opcode フィールド
/// * `handler`     - 処理ルーチン識別子
/// * `size`        - サイズ指定（.b/.w/.l など）
/// * `operands`    - 解析済み実効アドレスリスト
pub fn encode_insn(
    base_opcode: u16,
    handler: InsnHandler,
    size: SizeCode,
    operands: &[EffectiveAddress],
) -> Result<Vec<u8>, InsnError> {
    match handler {
        // ---- データ転送 ----
        InsnHandler::Move      => encode_move(size, operands),
        InsnHandler::MoveA     => encode_movea(size, operands),
        InsnHandler::MoveQ     => encode_moveq(operands),
        InsnHandler::MoveM     => encode_movem(size, operands),
        InsnHandler::MoveP     => encode_movep(size, operands),
        InsnHandler::Lea       => encode_lea(operands),
        InsnHandler::PeaJsrJmp => encode_peajsrjmp(base_opcode, operands),
        InsnHandler::JmpJsr    => encode_jmpjsr(base_opcode, operands),
        // ---- 算術 ----
        InsnHandler::SubAdd    => encode_subadd(base_opcode, size, operands),
        InsnHandler::SubAddQ   => encode_subaddq(base_opcode, size, operands),
        InsnHandler::SubAddI   => encode_subaddi(base_opcode, size, operands),
        InsnHandler::SbAdCpA   => encode_sbadcpa(base_opcode, size, operands),
        InsnHandler::SubAddX   => encode_subaddx(base_opcode, size, operands),
        InsnHandler::DivMul    => encode_divmul(base_opcode, operands),
        InsnHandler::NegNot    => encode_negnot(base_opcode, size, operands),
        InsnHandler::Clr       => encode_clr(base_opcode, size, operands),
        InsnHandler::Tst       => encode_tst(base_opcode, size, operands),
        InsnHandler::Ext       => encode_ext(size, operands),
        InsnHandler::Swap      => encode_swap(operands),
        InsnHandler::Exg       => encode_exg(operands),
        InsnHandler::Chk       => encode_chk(size, operands),
        InsnHandler::SAbcd     => encode_sabcd(base_opcode, operands),
        InsnHandler::DecInc    => encode_decinc(base_opcode, size, operands),
        // ---- 比較 ----
        InsnHandler::Cmp       => encode_cmp(base_opcode, size, operands),
        InsnHandler::CmpI      => encode_cmpi(base_opcode, size, operands),
        InsnHandler::CmpA      => encode_cmpa(base_opcode, size, operands),
        InsnHandler::CmpM      => encode_cmpm(base_opcode, size, operands),
        // ---- 論理 ----
        InsnHandler::OrAnd     => encode_orand(base_opcode, size, operands),
        InsnHandler::OrAndEorI => encode_orandeorimm(base_opcode, size, operands),
        InsnHandler::Eor       => encode_eor(base_opcode, size, operands),
        // ---- ビット操作 ----
        InsnHandler::BchClSt   => encode_bchclst(base_opcode, operands),
        InsnHandler::Btst      => encode_btst(operands),
        // ---- シフト/ローテート ----
        InsnHandler::SftRot    => encode_sftrot(base_opcode, size, operands),
        InsnHandler::Asl       => encode_asl(base_opcode, size, operands),
        // ---- 分岐 ----
        InsnHandler::Bcc       => encode_bcc(base_opcode, operands),
        InsnHandler::JBcc      => Err(InsnError::DeferToLinker),
        InsnHandler::DBcc      => encode_dbcc(base_opcode, operands),
        InsnHandler::Scc       => encode_scc(base_opcode, operands),
        // ---- フロー制御 ----
        InsnHandler::Link      => encode_link(size, operands),
        InsnHandler::Unlk      => encode_unlk(operands),
        InsnHandler::Trap      => encode_trap(operands),
        InsnHandler::StopRtd   => encode_stoprtd(base_opcode, operands),
        // ---- Phase 9: 68010+/68020+ 拡張命令 ----
        InsnHandler::ExtB         => encode_extb(operands),
        InsnHandler::Bkpt         => encode_bkpt(operands),
        InsnHandler::Trapcc       => encode_trapcc(base_opcode, size, operands),
        InsnHandler::BfChgClrSet  => encode_bitfield_1ea(base_opcode, operands),
        InsnHandler::BfExtFfo     => encode_bitfield_extract(base_opcode, operands),
        InsnHandler::BfIns        => encode_bfins(operands),
        InsnHandler::MovesInsn    => encode_moves(base_opcode, size, operands),
        InsnHandler::MoveC        => encode_movec(base_opcode, operands),
        InsnHandler::PackUnpk     => encode_packunpk(base_opcode, operands),
        InsnHandler::CasInsn      => encode_cas(base_opcode, size, operands),
        InsnHandler::CmpChk2      => encode_cmpchk2(base_opcode, size, operands),
        InsnHandler::Move16Insn   => encode_move16(operands),
        InsnHandler::CInvPushLP   => encode_cinvpush_lp(base_opcode, operands),
        InsnHandler::CInvPushA    => encode_cinvpush_a(base_opcode, operands),
        // 疑似命令・その他未実装
        _ => Err(InsnError::DeferToLinker),
    }
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressing::{parse_ea, EffectiveAddress};
    use crate::symbol::SymbolTable;
    use crate::options::cpu;

    fn sym() -> SymbolTable { SymbolTable::new(false) }

    fn parse(s: &str) -> EffectiveAddress {
        let t = sym();
        let mut pos = 0;
        parse_ea(s.as_bytes(), &mut pos, &t, cpu::C000).unwrap()
    }

    fn encode(handler: InsnHandler, opcode: u16, size: SizeCode, ops: Vec<&str>) -> Vec<u8> {
        let operands: Vec<EffectiveAddress> = ops.iter().map(|s| parse(s)).collect();
        encode_insn(opcode, handler, size, &operands).unwrap()
    }

    fn encode_ok(handler: InsnHandler, opcode: u16, size: SizeCode, ops: Vec<&str>) -> Option<Vec<u8>> {
        let operands: Vec<EffectiveAddress> = ops.iter().map(|s| parse(s)).collect();
        encode_insn(opcode, handler, size, &operands).ok()
    }

    // ---- no-operand (NOP / RTS etc.) ----

    #[test]
    fn test_nop() {
        // NOP: 0x4E71
        let v = encode(InsnHandler::Bcc, 0x4E71, SizeCode::None, vec![]);
        assert_eq!(v, vec![0x4E, 0x71]);
    }

    #[test]
    fn test_rts() {
        let v = encode(InsnHandler::Bcc, 0x4E75, SizeCode::None, vec![]);
        assert_eq!(v, vec![0x4E, 0x75]);
    }

    #[test]
    fn test_rte() {
        let v = encode(InsnHandler::Bcc, 0x4E73, SizeCode::None, vec![]);
        assert_eq!(v, vec![0x4E, 0x73]);
    }

    #[test]
    fn test_illegal() {
        let v = encode(InsnHandler::Bcc, 0x4AFC, SizeCode::None, vec![]);
        assert_eq!(v, vec![0x4A, 0xFC]);
    }

    // ---- MOVE ----

    #[test]
    fn test_move_b_dn_dn() {
        // MOVE.B D0, D1 → 0x1200
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0x12, 0x00]);
    }

    #[test]
    fn test_move_w_dn_dn() {
        // MOVE.W D0, D1 → 0x3200
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Word, vec!["d0", "d1"]);
        assert_eq!(v, vec![0x32, 0x00]);
    }

    #[test]
    fn test_move_l_dn_dn() {
        // MOVE.L D0, D1 → 0x2200
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Long, vec!["d0", "d1"]);
        assert_eq!(v, vec![0x22, 0x00]);
    }

    #[test]
    fn test_move_w_an_dn() {
        // MOVE.W A0, D1 → source=An(0)=0x08, dest=Dn(1)
        // Opcode: 0x3000 | (1<<9) | (0<<6) | 0x08 = 0x3208
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Word, vec!["a0", "d1"]);
        assert_eq!(v, vec![0x32, 0x08]);
    }

    #[test]
    fn test_move_l_imm_dn() {
        // MOVE.L #$12345678, D0 → 0x203C + 0x12345678
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Long, vec!["#$12345678", "d0"]);
        assert_eq!(v, vec![0x20, 0x3C, 0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_move_w_dspadr_dn() {
        // MOVE.W (4,a1), D2 → 0x3429 + 0x0004
        // src EA: (4,a1) = DSPADR|1 = 0x29
        // dest: D2 bits 11-9=010, mode=000 → bits 11-6 = 010_000 = 0x0400
        let v = encode(InsnHandler::Move, 0x0000, SizeCode::Word, vec!["(4,a1)", "d2"]);
        assert_eq!(v, vec![0x34, 0x29, 0x00, 0x04]);
    }

    // ---- MOVEA ----

    #[test]
    fn test_movea_w() {
        // MOVEA.W D0, A1 → 0x3240
        // 0011 001 001 000000 = 0x3240
        let v = encode(InsnHandler::MoveA, 0x2040, SizeCode::Word, vec!["d0", "a1"]);
        assert_eq!(v, vec![0x32, 0x40]);
    }

    #[test]
    fn test_movea_l() {
        // MOVEA.L D0, A1 → 0x2240
        // 0010 001 001 000000 = 0x2240
        let v = encode(InsnHandler::MoveA, 0x2040, SizeCode::Long, vec!["d0", "a1"]);
        assert_eq!(v, vec![0x22, 0x40]);
    }

    // ---- MOVEQ ----

    #[test]
    fn test_moveq() {
        // MOVEQ #1, D0 → 0x7001
        let v = encode(InsnHandler::MoveQ, 0x7000, SizeCode::Long, vec!["#1", "d0"]);
        assert_eq!(v, vec![0x70, 0x01]);
    }

    #[test]
    fn test_moveq_negative() {
        // MOVEQ #-1, D0 → 0x70FF
        let v = encode(InsnHandler::MoveQ, 0x7000, SizeCode::Long, vec!["#-1", "d0"]);
        assert_eq!(v, vec![0x70, 0xFF]);
    }

    #[test]
    fn test_moveq_range_error() {
        // 256 は 8 ビットに入らない → エラー
        let operands = vec![parse("#256"), parse("d0")];
        let result = encode_insn(0x7000, InsnHandler::MoveQ, SizeCode::Long, &operands);
        assert!(result.is_err());
    }

    // ---- ADD/SUB ----

    #[test]
    fn test_add_b_dn_dn() {
        // ADD.B D0, D1 → 0xD200 (src→dst, direction=0)
        // Actually ADD <ea>,Dn: base=0xD000, sz=00, Dn=1 in 11-9, EA=D0
        // 0xD000 | 0x00 | (1<<9) | 0 = 0xD200
        let v = encode(InsnHandler::SubAdd, 0xD000, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xD2, 0x00]);
    }

    #[test]
    fn test_add_w_dn_mem() {
        // ADD.W D0, (A1) → dir=1, base=0xD000|0x0100, sz=0x40, D0 in 11-9, (A1)=0x11
        // 0xD000 | 0x0100 | 0x40 | (0<<9) | 0x11 = 0xD151
        let v = encode(InsnHandler::SubAdd, 0xD000, SizeCode::Word, vec!["d0", "(a1)"]);
        assert_eq!(v, vec![0xD1, 0x51]);
    }

    #[test]
    fn test_sub_l_dn_dn() {
        // SUB.L D2, D3 → base=0x9000, dir=0, sz=0x80, D3 in 11-9, D2 EA=0x02
        // 0x9000 | 0x80 | (3<<9) | 2 = 0x9682
        let v = encode(InsnHandler::SubAdd, 0x9000, SizeCode::Long, vec!["d2", "d3"]);
        assert_eq!(v, vec![0x96, 0x82]);
    }

    // ---- ADDI/SUBI ----

    #[test]
    fn test_addi_b() {
        // ADDI.B #5, D0 → 0x0600 | 0x00 | 0x00, then #5 padded = 0x0005
        let v = encode(InsnHandler::SubAddI, 0x0600, SizeCode::Byte, vec!["#5", "d0"]);
        assert_eq!(v, vec![0x06, 0x00, 0x00, 0x05]);
    }

    #[test]
    fn test_subi_w() {
        // SUBI.W #$100, D1 → 0x0441, then 0x0100
        let v = encode(InsnHandler::SubAddI, 0x0400, SizeCode::Word, vec!["#$100", "d1"]);
        assert_eq!(v, vec![0x04, 0x41, 0x01, 0x00]);
    }

    // ---- ADDQ/SUBQ ----

    #[test]
    fn test_addq_b() {
        // ADDQ.B #4, D0 → 0x5800 | (4<<9) | 0x00 = 0x5880? No wait:
        // base=0x5000, qval=4, sz=0x00, EA=Dn(0)=0 → 0x5000|(4<<9)|0 = 0x5800
        let v = encode(InsnHandler::SubAddQ, 0x5000, SizeCode::Byte, vec!["#4", "d0"]);
        assert_eq!(v, vec![0x58, 0x00]);
    }

    #[test]
    fn test_subq_w() {
        // SUBQ.W #8, D0 → base=0x5100, qval=0 (8→0), sz=0x40, EA=Dn(0)
        // 0x5100 | (0<<9) | 0x40 | 0 = 0x5140
        let v = encode(InsnHandler::SubAddQ, 0x5100, SizeCode::Word, vec!["#8", "d0"]);
        assert_eq!(v, vec![0x51, 0x40]);
    }

    // ---- CMP ----

    #[test]
    fn test_cmp_b_dn_dn() {
        // CMP.B D0, D1 → 0xB000|0x00|(1<<9)|0 = 0xB200
        let v = encode(InsnHandler::Cmp, 0xB000, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xB2, 0x00]);
    }

    // ---- NEG/NOT/CLR/TST ----

    #[test]
    fn test_neg_b() {
        // NEG.B D0 → 0x4400 | 0x00 | 0x00 = 0x4400
        let v = encode(InsnHandler::NegNot, 0x4400, SizeCode::Byte, vec!["d0"]);
        assert_eq!(v, vec![0x44, 0x00]);
    }

    #[test]
    fn test_not_w() {
        // NOT.W D3 → 0x4600 | 0x40 | 0x03 = 0x4643
        let v = encode(InsnHandler::NegNot, 0x4600, SizeCode::Word, vec!["d3"]);
        assert_eq!(v, vec![0x46, 0x43]);
    }

    #[test]
    fn test_clr_l() {
        // CLR.L D0 → 0x4200 | 0x80 = 0x4280
        let v = encode(InsnHandler::Clr, 0x4200, SizeCode::Long, vec!["d0"]);
        assert_eq!(v, vec![0x42, 0x80]);
    }

    #[test]
    fn test_tst_w_mem() {
        // TST.W (A0) → 0x4A00 | 0x40 | 0x10 = 0x4A50
        let v = encode(InsnHandler::Tst, 0x4A00, SizeCode::Word, vec!["(a0)"]);
        assert_eq!(v, vec![0x4A, 0x50]);
    }

    // ---- EXT ----

    #[test]
    fn test_ext_w() {
        // EXT.W D0 → 0x4880
        let v = encode(InsnHandler::Ext, 0x4880, SizeCode::Word, vec!["d0"]);
        assert_eq!(v, vec![0x48, 0x80]);
    }

    #[test]
    fn test_ext_l() {
        // EXT.L D0 → 0x48C0
        let v = encode(InsnHandler::Ext, 0x4880, SizeCode::Long, vec!["d0"]);
        assert_eq!(v, vec![0x48, 0xC0]);
    }

    // ---- SWAP ----

    #[test]
    fn test_swap() {
        // SWAP D3 → 0x4843
        let v = encode(InsnHandler::Swap, 0x4840, SizeCode::Word, vec!["d3"]);
        assert_eq!(v, vec![0x48, 0x43]);
    }

    // ---- EXG ----

    #[test]
    fn test_exg_dn_dn() {
        // EXG D0, D1 → 0xC100 | (0<<9) | (0x08<<3)? No:
        // word = 0xC100 | (0<<9) | (0x08<<3) | 1 = 0xC100 | 0x0040 | 1 = 0xC141
        let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xC1, 0x41]);
    }

    #[test]
    fn test_exg_an_an() {
        // EXG A0, A1 → 0xC100 | (0<<9) | (0x09<<3) | (1) = 0xC100|0x48|1 = 0xC149
        let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["a0", "a1"]);
        assert_eq!(v, vec![0xC1, 0x49]);
    }

    #[test]
    fn test_exg_dn_an() {
        // EXG D0, A1 → 0xC100 | (0<<9) | (0x11<<3) | 1 = 0xC100|0x88|1 = 0xC189
        let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["d0", "a1"]);
        assert_eq!(v, vec![0xC1, 0x89]);
    }

    // ---- AND/OR/EOR ----

    #[test]
    fn test_and_b_dn_dn() {
        // AND.B D0, D1 → <ea>,Dn: 0xC000|(1<<9)|0x00|0 = 0xC200
        let v = encode(InsnHandler::OrAnd, 0xC000, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xC2, 0x00]);
    }

    #[test]
    fn test_or_w_mem_dn() {
        // OR.W (A0), D1 → 0x8000|0x40|(1<<9)|0x10 = 0x8250+0x10? Actually:
        // base=0x8000, dir=0, sz=0x40, D1 in 11-9=(1<<9)=0x0200, EA=(A0)=0x10
        // 0x8000|0x40|0x0200|0x10 = 0x8250
        let v = encode(InsnHandler::OrAnd, 0x8000, SizeCode::Word, vec!["(a0)", "d1"]);
        assert_eq!(v, vec![0x82, 0x50]);
    }

    #[test]
    fn test_eor_l_dn_dn() {
        // EOR.L D0, D1 → 0xB100|0x80|(0<<9)|1 = 0xB181
        let v = encode(InsnHandler::Eor, 0xB100, SizeCode::Long, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xB1, 0x81]);
    }

    // ---- SHIFT ----

    #[test]
    fn test_asr_b_imm_dn() {
        // ASR.B #1, D0 → 0xE000|(1<<9)|0x00|0 = 0xE200
        let v = encode(InsnHandler::SftRot, 0xE000, SizeCode::Byte, vec!["#1", "d0"]);
        assert_eq!(v, vec![0xE2, 0x00]);
    }

    #[test]
    fn test_lsl_w_dn_dn() {
        // LSL.W D1, D0 → 0xE108|0x40|0x20|(1<<9)|0 = ?
        // base=0xE108, &0xFFF8=0xE108, sz=0x40, bit5=0x20, D1=1<<9=0x200, D0=0
        // 0xE108 | 0x40 | 0x20 | 0x0200 = 0xE368
        let v = encode(InsnHandler::SftRot, 0xE108, SizeCode::Word, vec!["d1", "d0"]);
        assert_eq!(v, vec![0xE3, 0x68]);
    }

    #[test]
    fn test_ror_w_imm8_dn() {
        // ROR.W #8, D0 → 0xE018|(8→0)|0x40|0 = 0xE018|0x40|0 = 0xE058
        let v = encode(InsnHandler::SftRot, 0xE018, SizeCode::Word, vec!["#8", "d0"]);
        assert_eq!(v, vec![0xE0, 0x58]);
    }

    // ---- LEA ----

    #[test]
    fn test_lea() {
        // LEA (A0), A1 → 0x41C0 | (1<<9) | 0x10 = 0x43D0
        let v = encode(InsnHandler::Lea, 0x41C0, SizeCode::Long, vec!["(a0)", "a1"]);
        assert_eq!(v, vec![0x43, 0xD0]);
    }

    // ---- PEA ----

    #[test]
    fn test_pea() {
        // PEA (A0) → 0x4840 | 0x10 = 0x4850
        let v = encode(InsnHandler::PeaJsrJmp, 0x4840, SizeCode::Long, vec!["(a0)"]);
        assert_eq!(v, vec![0x48, 0x50]);
    }

    // ---- JMP/JSR ----

    #[test]
    fn test_jsr() {
        // JSR (A0) → 0x4E80 | 0x10 = 0x4E90
        let v = encode(InsnHandler::JmpJsr, 0x4E80, SizeCode::None, vec!["(a0)"]);
        assert_eq!(v, vec![0x4E, 0x90]);
    }

    #[test]
    fn test_jmp_abs() {
        // JMP $1234.w → 0x4EC0 | 0x38 = 0x4EF8, then 0x1234
        let v = encode(InsnHandler::JmpJsr, 0x4EC0, SizeCode::None, vec!["$1234.w"]);
        assert_eq!(v, vec![0x4E, 0xF8, 0x12, 0x34]);
    }

    // ---- ADDQ / SUBQ edge cases ----

    #[test]
    fn test_addq_8() {
        // ADDQ.W #8, D0 → qval=0, sz=0x40, base=0x5000
        // 0x5000|(0<<9)|0x40|0 = 0x5040
        let v = encode(InsnHandler::SubAddQ, 0x5000, SizeCode::Word, vec!["#8", "d0"]);
        assert_eq!(v, vec![0x50, 0x40]);
    }

    // ---- BTST / BSET ----

    #[test]
    fn test_btst_static() {
        // BTST #3, D0 → 0x0000|0x0800|0x00, then 0x0003
        let v = encode(InsnHandler::Btst, 0x0000, SizeCode::None, vec!["#3", "d0"]);
        assert_eq!(v, vec![0x08, 0x00, 0x00, 0x03]);
    }

    #[test]
    fn test_btst_dynamic() {
        // BTST D0, D1 → 0x0000|0x0100|(0<<9)|1 = 0x0101
        let v = encode(InsnHandler::Btst, 0x0000, SizeCode::None, vec!["d0", "d1"]);
        assert_eq!(v, vec![0x01, 0x01]);
    }

    #[test]
    fn test_bset_static() {
        // BSET #7, D3 → 0x00C0|0x0800|3, then 0x0007
        let v = encode(InsnHandler::BchClSt, 0x00C0, SizeCode::None, vec!["#7", "d3"]);
        assert_eq!(v, vec![0x08, 0xC3, 0x00, 0x07]);
    }

    // ---- TRAP ----

    #[test]
    fn test_trap() {
        // TRAP #1 → 0x4E41
        let v = encode(InsnHandler::Trap, 0x4E40, SizeCode::None, vec!["#1"]);
        assert_eq!(v, vec![0x4E, 0x41]);
    }

    // ---- STOP ----

    #[test]
    fn test_stop() {
        // STOP #$2700 → 0x4E72, 0x2700
        let v = encode(InsnHandler::StopRtd, 0x4E72, SizeCode::None, vec!["#$2700"]);
        assert_eq!(v, vec![0x4E, 0x72, 0x27, 0x00]);
    }

    // ---- UNLK ----

    #[test]
    fn test_unlk() {
        // UNLK A0 → 0x4E58
        let v = encode(InsnHandler::Unlk, 0x4E58, SizeCode::None, vec!["a0"]);
        assert_eq!(v, vec![0x4E, 0x58]);
    }

    // ---- ADDA/SUBA/CMPA ----

    #[test]
    fn test_adda_w() {
        // ADDA.W D0, A1 → 0xD0C0|(1<<9)|0 = 0xD2C0? No wait:
        // base=0xD0C0, size_bit=0 (word), An=1 in 11-9, src=D0=0
        // 0xD0C0|(1<<9)|0 = 0xD2C0
        let v = encode(InsnHandler::SbAdCpA, 0xD0C0, SizeCode::Word, vec!["d0", "a1"]);
        assert_eq!(v, vec![0xD2, 0xC0]);
    }

    #[test]
    fn test_adda_l() {
        // ADDA.L D0, A1 → 0xD0C0|0x100|(1<<9)|0 = 0xD3C0
        let v = encode(InsnHandler::SbAdCpA, 0xD0C0, SizeCode::Long, vec!["d0", "a1"]);
        assert_eq!(v, vec![0xD3, 0xC0]);
    }

    // ---- ADDX/SUBX ----

    #[test]
    fn test_addx_dn() {
        // ADDX.B D0, D1 → 0xD100|0x00|(1<<9)|0 = 0xD300
        let v = encode(InsnHandler::SubAddX, 0xD100, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xD3, 0x00]);
    }

    #[test]
    fn test_subx_predec() {
        // SUBX.W -(A0), -(A1) → 0x9100|0x40|0x08|(1<<9)|0 = ?
        // base=0x9100, sz=0x40, mode=0x08, Ax=1 in 11-9, Ay=0
        // 0x9100|0x40|0x08|0x0200|0 = 0x9348
        let v = encode(InsnHandler::SubAddX, 0x9100, SizeCode::Word, vec!["-(a0)", "-(a1)"]);
        assert_eq!(v, vec![0x93, 0x48]);
    }

    // ---- Scc ----

    #[test]
    fn test_st() {
        // ST D0 → 0x50C0 | 0x00 = 0x50C0
        let v = encode(InsnHandler::Scc, 0x50C0, SizeCode::Byte, vec!["d0"]);
        assert_eq!(v, vec![0x50, 0xC0]);
    }

    #[test]
    fn test_sne() {
        // SNE (A0) → 0x56C0 | 0x10 = 0x56D0
        let v = encode(InsnHandler::Scc, 0x56C0, SizeCode::Byte, vec!["(a0)"]);
        assert_eq!(v, vec![0x56, 0xD0]);
    }

    // ---- DEC/INC ----

    #[test]
    fn test_dec_b() {
        // DEC.B D0 → SUBQ #1, D0 = 0x5300|(1<<9)|0x00|0 = 0x5500
        let v = encode(InsnHandler::DecInc, 0x5300, SizeCode::Byte, vec!["d0"]);
        assert_eq!(v, vec![0x53, 0x00]);
    }

    // ---- EXG variant ----

    #[test]
    fn test_exg_an_dn() {
        // EXG A0, D1 (same as EXG D1, A0) → mode=0x11, Rx=D1=1, Ry=A0=0
        // 0xC100|(1<<9)|(0x11<<3)|0 = 0xC100|0x0200|0x0088 = 0xC388
        let v = encode(InsnHandler::Exg, 0xC100, SizeCode::Long, vec!["a0", "d1"]);
        // EXG An, Dn: rx=Dn, ry=An (swap)
        // For (A0, D1): operands[0]=A0 (An), operands[1]=D1 (Dn) → mode=0x11, rx=D1=1, ry=A0=0
        assert_eq!(v, vec![0xC3, 0x88]);
    }

    // ---- Branch DeferToLinker ----

    #[test]
    fn test_bra_defers() {
        let operands = vec![parse("$1000")];
        let result = encode_insn(0x6000, InsnHandler::Bcc, SizeCode::Word, &operands);
        assert_eq!(result, Err(InsnError::DeferToLinker));
    }

    // ---- MULU/DIVS ----

    #[test]
    fn test_mulu_w() {
        // MULU.W D0, D1 → 0xC0C0|(1<<9)|0 = 0xC2C0
        let v = encode(InsnHandler::DivMul, 0xC0C0, SizeCode::Word, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xC2, 0xC0]);
    }

    #[test]
    fn test_divs_w() {
        // DIVS.W D0, D1 → 0x81C0|(1<<9)|0 = 0x83C0
        let v = encode(InsnHandler::DivMul, 0x81C0, SizeCode::Word, vec!["d0", "d1"]);
        assert_eq!(v, vec![0x83, 0xC0]);
    }

    // ---- ABCD/SBCD ----

    #[test]
    fn test_abcd_dn() {
        // ABCD D0, D1 → 0xC100|(1<<9)|0x00|0 = 0xC300
        let v = encode(InsnHandler::SAbcd, 0xC100, SizeCode::Byte, vec!["d0", "d1"]);
        assert_eq!(v, vec![0xC3, 0x00]);
    }

    #[test]
    fn test_sbcd_predec() {
        // SBCD -(A0), -(A1) → 0x8100|(1<<9)|0x08|0 = 0x8308
        let v = encode(InsnHandler::SAbcd, 0x8100, SizeCode::Byte, vec!["-(a0)", "-(a1)"]);
        assert_eq!(v, vec![0x83, 0x08]);
    }

    // ---- CMPM ----

    #[test]
    fn test_cmpm_w() {
        // CMPM.W (A0)+, (A1)+ → 0xB108|0x40|(1<<9)|0 = 0xB348
        let v = encode(InsnHandler::CmpM, 0xB108, SizeCode::Word, vec!["(a0)+", "(a1)+"]);
        assert_eq!(v, vec![0xB3, 0x48]);
    }

    // ---- MOVEM ----

    #[test]
    fn test_movem_to_mem() {
        // MOVEM.W #0x00FF, (A0)
        // reg→mem: direction=0, sz=0, EA=(A0)=0x10, mask=0x00FF
        // opcode = 0x4880|0|0|0x10 = 0x4890, then mask=0x00FF
        use crate::expr::rpn::RPNToken;
        let operands = vec![
            EffectiveAddress::Immediate(vec![RPNToken::Value(0x00FF)]),
            parse("(a0)"),
        ];
        let result = encode_insn(0x4880, InsnHandler::MoveM, SizeCode::Word, &operands);
        assert!(result.is_ok());
        let v = result.unwrap();
        assert_eq!(v[0..2], [0x48, 0x90]);
        assert_eq!(v[2..4], [0x00, 0xFF]);
    }
}
