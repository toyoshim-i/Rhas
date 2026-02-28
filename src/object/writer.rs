/// HLK オブジェクトファイル書き出し
///
/// docs/hlk_object_format.md で定義された形式に従って書き出す。
/// HAS060.X の実際の出力形式に合わせた実装:
/// - ファイルサイズフィールド = ファイル全体のバイト数
/// - ファイル名・セクション名は偶数バイトにパディング（null 込みで奇数なら追加 null）
/// - 常に text/data/bss/stack の4セクションヘッダを出力
/// - コードボディは 20xx セクション切り替え + 10xx コードブロック形式

use super::ObjectCode;

/// null 終端文字列を偶数バイトになるようパディングして追加
fn push_str_even(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
    out.push(0x00);
    // name + null の長さが奇数なら追加 null でパディング
    if (s.len() + 1) % 2 != 0 {
        out.push(0x00);
    }
}

/// ObjectCode → HLK バイナリを生成する
pub fn write_hlk(obj: &ObjectCode) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);

    // ---- $D000: ファイルヘッダ ----
    out.push(0xD0);
    out.push(0x00);
    // total_size プレースホルダ（後で書き戻す）
    let total_size_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    // ソースファイル名（拡張子なし、偶数パディング付き null 終端）
    push_str_even(&mut out, &obj.source_name);

    // ---- $C0xx: セクションヘッダ ----
    for sect in &obj.sections {
        out.push(0xC0);
        out.push(sect.id);
        out.extend_from_slice(&sect.size.to_be_bytes());
        push_str_even(&mut out, sect.name().as_bytes());
    }

    // ---- $E001: requestファイル ----
    for req in &obj.request_files {
        out.push(0xE0);
        out.push(0x01);
        push_str_even(&mut out, req);
    }

    // ---- $B204: アラインメント情報（.align 使用時 / -g 指定時）----
    if obj.has_align || obj.has_debug_info {
        out.push(0xB2);
        out.push(0x04);
        let n = obj.max_align as u32;
        out.extend_from_slice(&n.to_be_bytes());
        // '*' + ソースファイル名（拡張子あり）+ '*'（偶数バイトパディング付き null 終端）
        let mut b204_str = Vec::new();
        b204_str.push(b'*');
        b204_str.extend_from_slice(&obj.source_file);
        b204_str.push(b'*');
        push_str_even(&mut out, &b204_str);
    }

    // ---- $B2xx: 外部シンボル ----
    for sym in &obj.ext_syms {
        out.push(0xB2);
        out.push(sym.kind);
        out.extend_from_slice(&sym.value.to_be_bytes());
        push_str_even(&mut out, &sym.name);
    }

    // ---- オブジェクトコード本体（20xx + 10xx 形式）----
    out.extend_from_slice(&obj.code_body);

    // ---- $0000: 終端 ----
    out.push(0x00);
    out.push(0x00);

    // total_size を書き戻す（HAS060.X 互換: ファイル全体のサイズ）
    let total_size = out.len() as u32;
    let ts = total_size.to_be_bytes();
    out[total_size_pos]     = ts[0];
    out[total_size_pos + 1] = ts[1];
    out[total_size_pos + 2] = ts[2];
    out[total_size_pos + 3] = ts[3];

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ExternalSymbol, ObjectCode, SectionInfo, sym_kind};

    #[test]
    fn test_minimal_hlk() {
        // move.b d0,d1 だけのオブジェクトファイル（HAS060.X 互換形式）
        let mut obj = ObjectCode::new(b"test".to_vec());
        // text section
        obj.sections.push(SectionInfo { id: 0x01, bytes: vec![0x12, 0x00], size: 2 });
        // data/bss/stack (empty)
        obj.sections.push(SectionInfo { id: 0x02, bytes: vec![], size: 0 });
        obj.sections.push(SectionInfo { id: 0x03, bytes: vec![], size: 0 });
        obj.sections.push(SectionInfo { id: 0x04, bytes: vec![], size: 0 });
        // code_body: 10 01 12 00
        obj.code_body = vec![0x10, 0x01, 0x12, 0x00];
        let hlk = write_hlk(&obj);

        // ファイル先頭: D0 00
        assert_eq!(&hlk[0..2], &[0xD0, 0x00]);
        // 最後2バイトが 00 00
        let len = hlk.len();
        assert_eq!(&hlk[len-2..], &[0x00, 0x00]);
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
            obj.sections.push(SectionInfo { id, bytes: vec![], size: 0 });
        }
        let hlk = write_hlk(&obj);
        // B2 FF が含まれるはず
        let found = hlk.windows(2).any(|w| w == [0xB2, 0xFF]);
        assert!(found);
    }
}
