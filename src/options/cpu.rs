// CPUタイプビット（cputype.equ より）
pub const C000: u16 = 1 << 8;
pub const C010: u16 = 1 << 9;
pub const C020: u16 = 1 << 10;
pub const C030: u16 = 1 << 11;
pub const C040: u16 = 1 << 12;
pub const C060: u16 = 1 << 13;
pub const CMMU: u16 = 1 << 14;
pub const CFPP: u16 = 1 << 15;
pub const C520: u16 = 1 << 0;
pub const C530: u16 = 1 << 1;
pub const C540: u16 = 1 << 2;

/// CPU番号をCPUタイプビットに変換する。
/// 返値: Some(CpuType) または None（不正な値）
pub fn cpu_number_to_type(n: u32) -> Option<crate::context::CpuType> {
    match n {
        68000 => Some(crate::context::CpuType::new(68000, C000)),
        68010 => Some(crate::context::CpuType::new(68010, C010)),
        68020 => Some(crate::context::CpuType::new(68020, C020)),
        68030 => Some(crate::context::CpuType::new(68030, C030)),
        68040 => Some(crate::context::CpuType::new(68040, C040)),
        68060 => Some(crate::context::CpuType::new(68060, C060)),
        5200 => Some(crate::context::CpuType::new(5200, C520)),
        5300 => Some(crate::context::CpuType::new(5300, C530)),
        5400 => Some(crate::context::CpuType::new(5400, C540)),
        _ => None,
    }
}
