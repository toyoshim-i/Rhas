//! 68000 命令エンコード（Phase 5）
//!
//! `encode_insn(base_opcode, handler, size, operands)` → `Vec<u8>`
//!
//! 入力はすでに解析済みの EffectiveAddress。
//! シンボル参照を含む EA は `InsnError::DeferToLinker` を返す。

pub mod arith;
pub mod cmp;
pub mod data;
pub mod flow;
pub mod fpu;
pub mod logic;
pub mod shift;

use crate::addressing::{
    encode::{encode_ea, EaEncoded, EncodeError},
    EffectiveAddress,
};
use crate::expr::eval_rpn;
use crate::expr::Rpn;
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
// 内部ユーティリティ (crate / module internal)
// ----------------------------------------------------------------

/// big-endian でワードを積む
pub(crate) fn push_word(bytes: &mut Vec<u8>, w: u16) {
    bytes.push((w >> 8) as u8);
    bytes.push(w as u8);
}

/// big-endian でロングワードを積む
pub(crate) fn push_long(bytes: &mut Vec<u8>, l: u32) {
    bytes.push((l >> 24) as u8);
    bytes.push((l >> 16) as u8);
    bytes.push((l >> 8) as u8);
    bytes.push(l as u8);
}

/// サイズコード → op_size (encode_ea の第2引数: 0=byte, 1=word, 2=long)
pub(crate) fn size_to_op_size(size: SizeCode) -> Result<u8, InsnError> {
    match size {
        SizeCode::Byte => Ok(0),
        SizeCode::Word => Ok(1),
        SizeCode::Long => Ok(2),
        _ => Err(InsnError::InvalidSize),
    }
}

/// サイズコード → bits 7-6 (00=byte, 01=word, 10=long)
pub(crate) fn size_field(size: SizeCode) -> Result<u16, InsnError> {
    match size {
        SizeCode::Byte => Ok(0x00),
        SizeCode::Word => Ok(0x40),
        SizeCode::Long => Ok(0x80),
        _ => Err(InsnError::InvalidSize),
    }
}

pub(crate) fn fpu_size_code(size: SizeCode) -> Result<u16, InsnError> {
    match size {
        SizeCode::Byte => Ok(6),
        SizeCode::Word => Ok(4),
        SizeCode::Long => Ok(0),
        SizeCode::Short => Ok(1), // single
        SizeCode::Double => Ok(5),
        SizeCode::Extend => Ok(2),
        SizeCode::Packed => Ok(3),
        _ => Err(InsnError::InvalidSize),
    }
}

pub(crate) fn eval_immediate_u8(rpn: &Rpn) -> Result<u8, InsnError> {
    match eval_rpn(rpn, 0, 0, 0, &|_| None) {
        Ok(v) if v.section == 0 && (0..=255).contains(&v.value) => Ok(v.value as u8),
        Ok(_) => Err(InsnError::OutOfRange {
            value: 256,
            min: 0,
            max: 255,
        }),
        Err(_) => Err(InsnError::DeferToLinker),
    }
}

/// RPN を定数評価する（シンボル参照があれば None）
pub(crate) fn eval_const(rpn: &Rpn) -> Option<i32> {
    if rpn.is_empty() {
        return Some(0);
    }
    match eval_rpn(rpn, 0, 0, 0, &|_| None) {
        Ok(v) if v.section == 0 => Some(v.value),
        _ => None,
    }
}

/// EncodeError → InsnError 変換
pub(crate) fn map_enc_err(e: EncodeError) -> InsnError {
    match e {
        EncodeError::DeferToLinker => InsnError::DeferToLinker,
        EncodeError::InvalidMode => InsnError::InvalidAddressingMode,
        EncodeError::DisplacementOutOfRange { value, bits } => {
            let half = 1i32 << (bits - 1);
            InsnError::OutOfRange {
                value,
                min: -half,
                max: half - 1,
            }
        }
    }
}

/// EA をエンコードする（失敗時は InsnError に変換）
pub(crate) fn enc(ea: &EffectiveAddress, op_size: u8) -> Result<EaEncoded, InsnError> {
    encode_ea(ea, op_size).map_err(map_enc_err)
}

/// DataReg なら番号を返す
pub(crate) fn data_reg(ea: &EffectiveAddress) -> Option<u8> {
    if let EffectiveAddress::DataReg(n) = ea {
        Some(*n)
    } else {
        None
    }
}

/// AddrReg なら番号を返す
pub(crate) fn addr_reg(ea: &EffectiveAddress) -> Option<u8> {
    if let EffectiveAddress::AddrReg(n) = ea {
        Some(*n)
    } else {
        None
    }
}

/// CAS2 の (Rn) オペランドからレジスタコードを取得
/// AddrRegInd(n) → 8+n, DataReg(n) → n
pub(crate) fn cas2_reg(ea: &EffectiveAddress) -> Option<u8> {
    match ea {
        EffectiveAddress::AddrRegInd(n) => Some(8 + n),
        EffectiveAddress::DataReg(n) => Some(*n),
        _ => None,
    }
}

/// Immediate の RPN を返す
pub(crate) fn imm_rpn(ea: &EffectiveAddress) -> Option<&Rpn> {
    if let EffectiveAddress::Immediate(rpn) = ea {
        Some(rpn)
    } else {
        None
    }
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
        InsnHandler::Move => data::encode_move(size, operands),
        InsnHandler::MoveA => data::encode_movea(size, operands),
        InsnHandler::MoveQ => data::encode_moveq(operands),
        InsnHandler::MoveM => data::encode_movem(size, operands),
        InsnHandler::MoveP => data::encode_movep(size, operands),
        InsnHandler::Lea => data::encode_lea(operands),
        InsnHandler::PeaJsrJmp => data::encode_peajsrjmp(base_opcode, operands),
        InsnHandler::JmpJsr => data::encode_jmpjsr(base_opcode, operands),
        // ---- 算術 ----
        InsnHandler::SubAdd => arith::encode_subadd(base_opcode, size, operands),
        InsnHandler::SubAddQ => arith::encode_subaddq(base_opcode, size, operands),
        InsnHandler::SubAddI => arith::encode_subaddi(base_opcode, size, operands),
        InsnHandler::SbAdCpA => arith::encode_sbadcpa(base_opcode, size, operands),
        InsnHandler::SubAddX => arith::encode_subaddx(base_opcode, size, operands),
        InsnHandler::DivMul => arith::encode_divmul(base_opcode, size, operands),
        InsnHandler::NegNot => arith::encode_negnot(base_opcode, size, operands),
        InsnHandler::Clr => arith::encode_clr(base_opcode, size, operands),
        InsnHandler::Tst => arith::encode_tst(base_opcode, size, operands),
        InsnHandler::Ext => arith::encode_ext(size, operands),
        InsnHandler::Swap => arith::encode_swap(operands),
        InsnHandler::Exg => logic::encode_exg(operands),
        InsnHandler::Chk => logic::encode_chk(size, operands),
        InsnHandler::SAbcd => arith::encode_sabcd(base_opcode, operands),
        InsnHandler::DecInc => arith::encode_decinc(base_opcode, size, operands),
        // ---- 比較 ----
        InsnHandler::Cmp => cmp::encode_cmp(base_opcode, size, operands),
        InsnHandler::CmpI => cmp::encode_cmpi(base_opcode, size, operands),
        InsnHandler::CmpA => cmp::encode_cmpa(base_opcode, size, operands),
        InsnHandler::CmpM => cmp::encode_cmpm(base_opcode, size, operands),
        // ---- 論理 ----
        InsnHandler::OrAnd => logic::encode_orand(base_opcode, size, operands),
        InsnHandler::OrAndEorI => logic::encode_orandeorimm(base_opcode, size, operands),
        InsnHandler::Eor => logic::encode_eor(base_opcode, size, operands),
        // ---- ビット操作 ----
        InsnHandler::BchClSt => logic::encode_bchclst(base_opcode, operands),
        InsnHandler::Btst => logic::encode_btst(operands),
        // ---- シフト/ローテート ----
        InsnHandler::SftRot => shift::encode_sftrot(base_opcode, size, operands),
        InsnHandler::Asl => shift::encode_asl(base_opcode, size, operands),
        // ---- 分岐 ----
        InsnHandler::Bcc => flow::encode_bcc(base_opcode, operands),
        InsnHandler::JBcc => Err(InsnError::DeferToLinker),
        InsnHandler::DBcc => flow::encode_dbcc(base_opcode, operands),
        InsnHandler::Scc => flow::encode_scc(base_opcode, operands),
        // ---- フロー制御 ----
        InsnHandler::Link => flow::encode_link(size, operands),
        InsnHandler::Unlk => flow::encode_unlk(operands),
        InsnHandler::Trap => flow::encode_trap(operands),
        InsnHandler::StopRtd => flow::encode_stoprtd(base_opcode, operands),
        // ---- Phase 9: 68010+/68020+ 拡張命令 ----
        InsnHandler::ExtB => flow::encode_extb(operands),
        InsnHandler::Bkpt => flow::encode_bkpt(operands),
        InsnHandler::Trapcc => flow::encode_trapcc(base_opcode, size, operands),
        InsnHandler::BfChgClrSet => flow::encode_bitfield_1ea(base_opcode, operands),
        InsnHandler::BfExtFfo => flow::encode_bitfield_extract(base_opcode, operands),
        InsnHandler::BfIns => flow::encode_bfins(operands),
        InsnHandler::MovesInsn => flow::encode_moves(base_opcode, size, operands),
        InsnHandler::MoveC => flow::encode_movec(base_opcode, operands),
        InsnHandler::PackUnpk => flow::encode_packunpk(base_opcode, operands),
        InsnHandler::CasInsn => flow::encode_cas(base_opcode, size, operands),
        InsnHandler::Cas2Insn => flow::encode_cas2(size, operands),
        InsnHandler::DivSlUl => flow::encode_divsl_ul(base_opcode, operands),
        InsnHandler::CmpChk2 => flow::encode_cmpchk2(base_opcode, size, operands),
        InsnHandler::Move16Insn => flow::encode_move16(operands),
        InsnHandler::CInvPushLP => flow::encode_cinvpush_lp(base_opcode, operands),
        InsnHandler::CInvPushA => flow::encode_cinvpush_a(base_opcode, operands),
        // ---- FPU ----
        InsnHandler::FMove => fpu::encode_fmove(base_opcode, size, operands),
        InsnHandler::FMoveM => fpu::encode_fmovem(base_opcode, size, operands),
        InsnHandler::FMoveCr => fpu::encode_fmovecr(base_opcode, size, operands),
        InsnHandler::FSinCos => fpu::encode_fsincos(base_opcode, size, operands),
        InsnHandler::FArith => fpu::encode_fop2(base_opcode, size, operands),
        InsnHandler::FCmp => fpu::encode_fop2(base_opcode, size, operands),
        InsnHandler::FTst => fpu::encode_ftst(base_opcode, size, operands),
        InsnHandler::FNop => fpu::encode_fnop(base_opcode, operands),
        InsnHandler::FSave => fpu::encode_fsave_frestore(base_opcode, operands),
        InsnHandler::FRestore => fpu::encode_fsave_frestore(base_opcode, operands),
        InsnHandler::FBcc => Err(InsnError::DeferToLinker),
        InsnHandler::FDBcc => Err(InsnError::DeferToLinker),
        // 疑似命令・その他未実装
        _ => Err(InsnError::DeferToLinker),
    }
}

// ----------------------------------------------------------------
// テスト
// ----------------------------------------------------------------

#[cfg(test)]
mod tests;
