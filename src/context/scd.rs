/// SCD 拡張シンボル一時データ（オリジナルの SCDTEMP 相当）
#[derive(Debug, Clone)]
pub struct ScdTemp {
    pub name: Vec<u8>,
    pub attrib: u8,
    pub value: u32,
    pub section: i16,
    pub scl: u8,
    pub type_code: u16,
    pub size: u32,
    pub dim: [u16; 4],
    pub is_long: bool,
}

impl Default for ScdTemp {
    fn default() -> Self {
        Self {
            name: Vec::new(),
            attrib: 0,
            value: 0,
            // HAS の scdtempclr に合わせる（SCD_SECT = -2）
            section: -2,
            scl: 0,
            type_code: 0,
            size: 0,
            dim: [0; 4],
            is_long: false,
        }
    }
}
