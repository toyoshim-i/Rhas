#![allow(dead_code)]
/// CPU型情報（cpu_number と cpu_type を統一）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuType {
    /// CPU番号（68000, 68010, 68020, ..., 68060）
    pub number: u32,
    /// CPUタイプビット（has060xx形式: 0x0001=C000, 0x0100=C010, ...）
    pub features: u16,
}

impl CpuType {
    /// 新しい CpuType を生成
    pub const fn new(number: u32, features: u16) -> Self {
        CpuType { number, features }
    }

    /// デフォルト値（68000, no features）
    pub const fn default_68000() -> Self {
        CpuType {
            number: 68000,
            features: 0x0100,
        }
    }

    /// 68010 CPU
    pub const fn cpu_68010() -> Self {
        CpuType {
            number: 68010,
            features: 0x0200,
        }
    }

    /// 68020 CPU
    pub const fn cpu_68020() -> Self {
        CpuType {
            number: 68020,
            features: 0x0400,
        }
    }

    /// CPU番号が古い世代か判定
    pub fn is_older_than_020(&self) -> bool {
        self.number < 68020
    }

    /// CPU番号が68060以降か判定
    #[allow(dead_code)]
    pub fn supports_060_extended(&self) -> bool {
        self.number >= 68060
    }
}
