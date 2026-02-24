/// HLK オブジェクトファイル書き出し
///
/// docs/hlk_object_format.md で定義された形式に従って書き出す。

use super::{ObjectCode, SectionInfo};

/// ObjectCode → HLK バイナリを生成する
pub fn write_hlk(obj: &ObjectCode) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);

    // ---- $D000: ファイルヘッダ ----
    // record code
    out.push(0xD0);
    out.push(0x00);
    // body_size プレースホルダ（後で書き戻す）
    let body_size_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    // ソースファイル名（拡張子なし + null 終端）
    out.extend_from_slice(&obj.source_name);
    out.push(0x00);

    // ---- $C0xx: セクションヘッダ ----
    for sect in &obj.sections {
        out.push(0xC0);
        out.push(sect.id);
        let size_bytes = sect.size.to_be_bytes();
        out.extend_from_slice(&size_bytes);
        out.extend_from_slice(sect.name().as_bytes());
        out.push(0x00);
    }

    // ---- $B204: アラインメント情報（.align 使用時）----
    if obj.has_align {
        out.push(0xB2);
        out.push(0x04);
        let n = obj.max_align as u32;
        out.extend_from_slice(&n.to_be_bytes());
        // '*' + ソースファイル名 + '*' + null
        out.push(b'*');
        out.extend_from_slice(&obj.source_name);
        out.push(b'*');
        out.push(0x00);
    }

    // ---- $B2xx: 外部シンボル ----
    for sym in &obj.ext_syms {
        out.push(0xB2);
        out.push(sym.kind as u8);
        out.extend_from_slice(&sym.value.to_be_bytes());
        out.extend_from_slice(&sym.name);
        out.push(0x00);
    }

    // ---- オブジェクトコード本体 ----
    // セクションのコードを順番に出力（text → data のみ; bss/stack はサイズのみ）
    for sect in &obj.sections {
        out.extend_from_slice(&sect.bytes);
    }

    // ---- $0000: 終端 ----
    out.push(0x00);
    out.push(0x00);

    // body_size を書き戻す
    // "このフィールドの直後から $0000 終端の直後まで"
    let body_size = (out.len() - body_size_pos - 4) as u32;
    let bs = body_size.to_be_bytes();
    out[body_size_pos]     = bs[0];
    out[body_size_pos + 1] = bs[1];
    out[body_size_pos + 2] = bs[2];
    out[body_size_pos + 3] = bs[3];

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ExternalSymbol, ObjectCode, SectionInfo, SymKind};

    #[test]
    fn test_minimal_hlk() {
        // move.b d0,d1 だけのオブジェクトファイル
        let mut obj = ObjectCode::new(b"test".to_vec());
        obj.sections.push(SectionInfo {
            id: 0x01,
            bytes: vec![0x12, 0x00],
            size: 2,
        });
        let hlk = write_hlk(&obj);

        // ファイル先頭: D0 00
        assert_eq!(&hlk[0..2], &[0xD0, 0x00]);
        // ソース名の直後に C0 01 セクションヘッダがある
        // "test\0" = 5 bytes → offset 2+4+5 = 11 → C0 01
        assert_eq!(hlk[11], 0xC0);
        assert_eq!(hlk[12], 0x01);
        // 最後2バイトが 00 00
        let len = hlk.len();
        assert_eq!(&hlk[len-2..], &[0x00, 0x00]);
    }

    #[test]
    fn test_xref_symbol() {
        let mut obj = ObjectCode::new(b"src".to_vec());
        obj.ext_syms.push(ExternalSymbol {
            kind: SymKind::XRef,
            value: 1,
            name: b"printf".to_vec(),
        });
        obj.sections.push(SectionInfo {
            id: 0x01,
            bytes: vec![],
            size: 0,
        });
        let hlk = write_hlk(&obj);
        // B2 FF が含まれるはず
        let found = hlk.windows(2).any(|w| w == [0xB2, 0xFF]);
        assert!(found);
    }
}
