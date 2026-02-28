/// 中間コードレコード（tmpcode.equ の T_* コードに対応）
///
/// オリジナルはテンポラリファイルに書き出すバイナリコードだが、
/// Rust版はメモリ上の Vec<TempRecord> として表現する。

use crate::addressing::EffectiveAddress;
use crate::expr::Rpn;
use crate::symbol::types::{InsnHandler, SizeCode};

/// 中間コードレコード
#[derive(Debug, Clone)]
pub enum TempRecord {
    /// 解決済みバイト列（Pass1で完全にエンコードできた命令／データ）
    Const(Vec<u8>),

    /// 未解決命令（シンボル参照を含む EA がある命令）
    /// Pass3 でシンボル値が確定した後に再エンコードする。
    DeferredInsn {
        base: u16,
        handler: InsnHandler,
        size: SizeCode,
        ops: Vec<EffectiveAddress>,
        /// Pass1 で推定した命令バイト数（ロケーションカウンタ計算用）
        byte_size: u32,
    },

    /// 分岐命令（Bcc/BRA/BSR）
    /// ターゲットは RPN 式で保持し、Pass3 でオフセット計算する。
    /// デフォルトはワード形式（4 バイト: 2 オペコード + 2 オフセット）。
    Branch {
        opcode: u16,
        target: Rpn,
        /// サイズ指定（None = 自動/デフォルトはワード）
        req_size: Option<SizeCode>,
        /// Pass2 後の実効サイズ（None = ワード）
        cur_size: Option<SizeCode>,
        /// Pass2 により分岐命令が削除された（直後への bra/bcc）
        suppressed: bool,
    },

    /// .dc データ（式を含むため Pass3 で評価）
    /// size: バイト数（1=byte, 2=word, 4=long）
    Data { size: u8, rpn: Rpn },

    /// .ds（スペース予約）
    /// BSS セクションでは 0 クリア不要（サイズのみ）
    Ds { byte_count: u32 },

    /// .align（アライン調整）
    /// n: アライン値（2^n バイト境界）
    /// pad: パディングバイト値（テキストセクションでは 0x4E71=NOP）
    Align { n: u8, pad: u16, section: u8 },

    /// ラベル定義記録（Pass2 でロケーション値の更新に使う）
    LabelDef { name: Vec<u8>, section: u8, offset: u32 },

    /// セクション変更（.text/.data/.bss/.stack）
    SectChange { id: u8 },

    /// .org（ロケーションカウンタ直接指定）
    Org { value: u32 },

    /// .xdef（外部定義）
    XDef { name: Vec<u8> },

    /// .xref（外部参照）
    XRef { name: Vec<u8> },

    /// .globl（外部参照/定義）
    Globl { name: Vec<u8> },

    /// .end
    End,

    /// .cpu（CPU 変更）
    Cpu { number: u32, cpu_type: u16 },

    /// PRNリストファイル用ソース行情報（-p オプション有効時のみ挿入）
    LineInfo { line_num: u32, text: Vec<u8>, is_macro: bool },
}

impl TempRecord {
    /// ロケーションカウンタの進み量（バイト数）
    ///
    /// `Align` は実行時に決まるため 0 を返す（pass3 で計算）。
    pub fn byte_size(&self) -> u32 {
        match self {
            TempRecord::Const(b)         => b.len() as u32,
            TempRecord::DeferredInsn { byte_size, .. } => *byte_size,
            TempRecord::Branch { cur_size, suppressed, .. } => {
                if *suppressed { 0 } else { branch_word_size(*cur_size) }
            }
            TempRecord::Data { size, .. } => *size as u32,
            TempRecord::Ds { byte_count } => *byte_count,
            TempRecord::Align { .. }     => 0,
            _                            => 0,
        }
    }
}

/// 分岐命令のデフォルトバイト数
/// .s → 2, .l → 6, その他（.w / デフォルト）→ 4
pub fn branch_word_size(req: Option<SizeCode>) -> u32 {
    match req {
        Some(SizeCode::Short) => 2,
        Some(SizeCode::Long)  => 6,
        _                     => 4,
    }
}
