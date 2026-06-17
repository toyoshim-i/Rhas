use super::*;
use crate::object::{sym_kind, ExternalSymbol, ObjectCode, SectionInfo};

#[test]
fn test_minimal_hlk() {
    // move.b d0,d1 だけのオブジェクトファイル（HAS060.X 互換形式）
    let mut obj = ObjectCode::new(b"test".to_vec());
    // text section
    obj.sections.push(SectionInfo {
        id: 0x01,
        bytes: vec![0x12, 0x00],
        size: 2,
    });
    // data/bss/stack (empty)
    obj.sections.push(SectionInfo {
        id: 0x02,
        bytes: vec![],
        size: 0,
    });
    obj.sections.push(SectionInfo {
        id: 0x03,
        bytes: vec![],
        size: 0,
    });
    obj.sections.push(SectionInfo {
        id: 0x04,
        bytes: vec![],
        size: 0,
    });
    // code_body: 10 01 12 00
    obj.code_body = vec![0x10, 0x01, 0x12, 0x00];
    let hlk = write_hlk(&obj);

    // ファイル先頭: D0 00
    assert_eq!(&hlk[0..2], &[0xD0, 0x00]);
    // 最後2バイトが 00 00
    let len = hlk.len();
    assert_eq!(&hlk[len - 2..], &[0x00, 0x00]);
    // サイズフィールドが total size に一致
    let stored_size = u32::from_be_bytes([hlk[2], hlk[3], hlk[4], hlk[5]]);
    assert_eq!(stored_size, len as u32);
    // "test\0\0" (偶数パディング: "test"=4バイト → 4+1=5(奇数) → +1 → 6バイト)
    assert_eq!(&hlk[6..12], b"test\0\0");
}

#[test]
fn test_xref_symbol() {
    let mut obj = ObjectCode::new(b"src".to_vec());
    obj.ext_syms.push(ExternalSymbol {
        kind: sym_kind::XREF,
        value: 1,
        name: b"printf".to_vec(),
    });
    // 4 sections
    for id in 1u8..=4 {
        obj.sections.push(SectionInfo {
            id,
            bytes: vec![],
            size: 0,
        });
    }
    let hlk = write_hlk(&obj);
    // B2 FF が含まれるはず
    let found = hlk.windows(2).any(|w| w == [0xB2, 0xFF]);
    assert!(found);
}
