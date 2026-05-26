/// セクション番号（has.equ: SECT_TEXT〜SECT_RLSTACK）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Section {
    Text    = 0x01,
    Data    = 0x02,
    Bss     = 0x03,
    Stack   = 0x04,
    Rdata   = 0x05,
    Rbss    = 0x06,
    Rstack  = 0x07,
    Rldata  = 0x08,
    Rlbss   = 0x09,
    Rlstack = 0x0A,
}

impl Section {
    /// セクション名文字列（HLKオブジェクトファイルに書き込む名前）
    pub fn name(self) -> &'static str {
        match self {
            Section::Text    => "text",
            Section::Data    => "data",
            Section::Bss     => "bss",
            Section::Stack   => "stack",
            Section::Rdata   => "rdata",
            Section::Rbss    => "rbss",
            Section::Rstack  => "rstack",
            Section::Rldata  => "rldata",
            Section::Rlbss   => "rlbss",
            Section::Rlstack => "rlstack",
        }
    }
}

/// セクション数（相対セクションを含む最大）
pub const N_SECTIONS: usize = 10;
