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

/// 外部シンボル種別
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

/// 外部シンボル情報（$B2xx レコード）
#[derive(Debug, Clone)]
pub struct ExternalSymbol {
    pub kind:  SymKind,
    pub value: u32,
    pub name:  Vec<u8>,
}

/// オブジェクトコード（Pass3 の出力）
#[derive(Debug)]
pub struct ObjectCode {
    /// ソースファイル名（拡張子なし）
    pub source_name: Vec<u8>,
    /// セクション情報（非空セクションのみ）
    pub sections: Vec<SectionInfo>,
    /// 外部シンボル
    pub ext_syms: Vec<ExternalSymbol>,
    /// リクエストファイル名（.request 疑似命令）
    pub request_files: Vec<Vec<u8>>,
    /// アライン使用フラグ
    pub has_align: bool,
    /// 最大アライン値（2^n の n）
    pub max_align: u8,
}

impl ObjectCode {
    pub fn new(source_name: Vec<u8>) -> Self {
        ObjectCode {
            source_name,
            sections: Vec::new(),
            ext_syms: Vec::new(),
            request_files: Vec::new(),
            has_align: false,
            max_align: 0,
        }
    }
}
