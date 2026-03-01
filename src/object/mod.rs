#![allow(dead_code)]
/// HLK オブジェクトファイル形式の型定義
///
/// docs/hlk_object_format.md 参照。

pub mod writer;

/// セクション情報
#[derive(Debug, Clone)]
pub struct SectionInfo {
    /// セクション番号（1=text, 2=data, 3=bss, 4=stack, ...）
    pub id: u8,
    /// セクションバイト列（BSS/Stack は空、サイズのみ）
    pub bytes: Vec<u8>,
    /// セクションサイズ（BSS/Stack は bytes.len() と異なる場合がある）
    pub size: u32,
}

impl SectionInfo {
    pub fn name(&self) -> &'static str {
        match self.id {
            0x01 => "text",
            0x02 => "data",
            0x03 => "bss",
            0x04 => "stack",
            0x05 => "rdata",
            0x06 => "rbss",
            0x07 => "rstack",
            0x08 => "rldata",
            0x09 => "rlbss",
            0x0A => "rlstack",
            _    => "unknown",
        }
    }
}

/// 外部シンボル種別定数（$B2xx レコードの xx 値）
///
/// セクション番号（0x00〜0x0A）または特殊種別（0xFA〜0xFF）。
/// HAS は定義済みシンボルには実際のセクション番号を使用する。
pub mod sym_kind {
    /// 絶対（オフセットセクション定義シンボル）
    pub const ABS:     u8 = 0x00;
    /// textセクション定義
    pub const TEXT:    u8 = 0x01;
    /// dataセクション定義
    pub const DATA:    u8 = 0x02;
    /// bssセクション定義
    pub const BSS:     u8 = 0x03;
    /// stackセクション定義
    pub const STACK:   u8 = 0x04;
    /// グローバル（外部参照/定義）
    pub const GLOBL:   u8 = 0xFA;
    /// 外部定義（.xdef）
    pub const XDEF:    u8 = 0xFB;
    /// コモンエリア（64KB以上相対）
    pub const RL_COMM: u8 = 0xFC;
    /// コモンエリア（64KB以内相対）
    pub const R_COMM:  u8 = 0xFD;
    /// コモンエリア
    pub const COMM:    u8 = 0xFE;
    /// 外部参照
    pub const XREF:    u8 = 0xFF;
}

/// 外部シンボル情報（$B2xx レコード）
/// kind: 0x00〜0x0A = セクション番号（定義済みシンボル）、0xFA〜0xFF = 特殊種別
#[derive(Debug, Clone)]
pub struct ExternalSymbol {
    pub kind:  u8,
    pub value: u32,
    pub name:  Vec<u8>,
}

/// SCDデバッグイベント
#[derive(Debug, Clone)]
pub enum ScdEvent {
    Ln { line: u16, location: u32, section: u8 },
    Val { value: u32, section: i16 },
    Tag { name: Vec<u8> },
    Endef {
        name: Vec<u8>,
        attrib: u8,
        value: u32,
        section: i16,
        scl: u8,
        type_code: u16,
        size: u32,
        dim: [u16; 4],
        is_long: bool,
    },
    FuncEnd { location: u32, section: u8 },
}

/// 旧 SymKind 互換エイリアス（既存テストとの互換性維持）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SymKind {
    Globl  = 0xFA,
    XDef   = 0xFB,
    RLComm = 0xFC,
    RComm  = 0xFD,
    Comm   = 0xFE,
    XRef   = 0xFF,
}

/// オブジェクトコード（Pass3 の出力）
#[derive(Debug)]
pub struct ObjectCode {
    /// ソースファイル名（拡張子なし、$D000 ヘッダ用）
    pub source_name: Vec<u8>,
    /// ソースファイル名（拡張子あり、$B204 レコード用）
    pub source_file: Vec<u8>,
    /// SCD 用ソースファイル名（`.file`。未指定時は source_file と同じ）
    pub scd_file: Vec<u8>,
    /// セクション情報（常に4セクション: text/data/bss/stack）
    pub sections: Vec<SectionInfo>,
    /// 外部シンボル
    pub ext_syms: Vec<ExternalSymbol>,
    /// リクエストファイル名（.request 疑似命令）
    pub request_files: Vec<Vec<u8>>,
    /// アライン使用フラグ
    pub has_align: bool,
    /// SCDデバッグ情報出力フラグ（-g）
    pub has_debug_info: bool,
    /// SCD疑似命令が有効化されたか（`.file` 検出後）
    pub scd_enabled: bool,
    /// 最大アライン値（2^n の n）
    pub max_align: u8,
    /// HLK コード本体（20xx セクション切り替え + 10xx コードブロック）
    pub code_body: Vec<u8>,
    /// 収集済みSCDイベント
    pub scd_events: Vec<ScdEvent>,
}

impl ObjectCode {
    pub fn new(source_name: Vec<u8>) -> Self {
        let source_file = source_name.clone();
        let scd_file = source_file.clone();
        ObjectCode {
            source_name,
            source_file,
            scd_file,
            sections: Vec::new(),
            ext_syms: Vec::new(),
            request_files: Vec::new(),
            has_align: false,
            has_debug_info: false,
            scd_enabled: false,
            max_align: 0,
            code_body: Vec::new(),
            scd_events: Vec::new(),
        }
    }
}
