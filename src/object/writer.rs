/// HLK オブジェクトファイル書き出し
///
/// docs/hlk_object_format.md で定義された形式に従って書き出す。
/// HAS060.X の実際の出力形式に合わせた実装:
/// - ファイルサイズフィールド = ファイル全体のバイト数
/// - ファイル名・セクション名は偶数バイトにパディング（null 込みで奇数なら追加 null）
/// - 常に text/data/bss/stack の4セクションヘッダを出力
/// - コードボディは 20xx セクション切り替え + 10xx コードブロック形式

use super::{ObjectCode, ScdEvent};

/// null 終端文字列を偶数バイトになるようパディングして追加
fn push_str_even(out: &mut Vec<u8>, s: &[u8]) {
    out.extend_from_slice(s);
    out.push(0x00);
    // name + null の長さが奇数なら追加 null でパディング
    if (s.len() + 1) % 2 != 0 {
        out.push(0x00);
    }
}

#[derive(Default, Clone)]
struct ScdEntry {
    name: [u8; 8],
    value: u32,
    section: i16,
    type_code: u16,
    scl: u8,
    len: u8,
    tag: u32,
    size: u32,
    dim0: u16,
    dim1: u16,
    next: u32,
}

fn fill_scd_name(dst: &mut [u8; 8], s: &[u8]) {
    let n = s.len().min(8);
    dst[..n].copy_from_slice(&s[..n]);
}

fn set_file_bytes(ent: &mut ScdEntry, s: &[u8]) {
    let mut buf = [0u8; 12];
    let n = s.len().min(12);
    buf[..n].copy_from_slice(&s[..n]);
    ent.tag = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    ent.size = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    ent.dim0 = u16::from_be_bytes([buf[8], buf[9]]);
    ent.dim1 = u16::from_be_bytes([buf[10], buf[11]]);
}

fn push_scd_entry(out: &mut Vec<u8>, e: &ScdEntry) {
    out.extend_from_slice(&e.name);
    out.extend_from_slice(&e.value.to_be_bytes());
    out.extend_from_slice(&(e.section as u16).to_be_bytes());
    out.extend_from_slice(&e.type_code.to_be_bytes());
    out.push(e.scl);
    out.push(e.len);
    out.extend_from_slice(&e.tag.to_be_bytes());
    out.extend_from_slice(&e.size.to_be_bytes());
    out.extend_from_slice(&e.dim0.to_be_bytes());
    out.extend_from_slice(&e.dim1.to_be_bytes());
    out.extend_from_slice(&e.next.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // 未使用
}

fn write_scd_footer(out: &mut Vec<u8>, obj: &ObjectCode) {
    if !obj.has_debug_info {
        return;
    }

    let mut lines: Vec<(u32, u16)> = Vec::new();
    let mut entries: Vec<ScdEntry> = Vec::new();
    let text_size = obj.sections.first().map(|s| s.size).unwrap_or(0);
    let mut exname_data: Vec<u8> = Vec::new();
    if obj.scd_file.len() > 14 {
        exname_data.extend_from_slice(&obj.scd_file);
        exname_data.push(0);
        if (exname_data.len() & 1) != 0 {
            exname_data.push(0);
        }
    }

    // HAS の既定エントリに合わせ、.file/.text/.data/.bss を最小構成で出力する。
    let mut file_ent = ScdEntry {
        value: if exname_data.is_empty() { 0 } else { 14 },
        section: -2,
        scl: 0x67,
        len: 1,
        ..Default::default()
    };
    fill_scd_name(&mut file_ent.name, b".file");
    set_file_bytes(&mut file_ent, &obj.scd_file);
    entries.push(file_ent);

    // HAS の scdout0 相当: 関数/.bf/.ef の雛形を追加する。
    let mut func_ent = ScdEntry {
        value: 0,
        section: 1,
        type_code: 0x0020,
        scl: 0x03,
        len: 1,
        size: text_size,
        next: 8,
        ..Default::default()
    };
    fill_scd_name(&mut func_ent.name, &obj.source_name);
    entries.push(func_ent);

    let mut bf_ent = ScdEntry {
        value: 0,
        section: 1,
        scl: 0x65,
        len: 1,
        size: 1,
        next: 6,
        ..Default::default()
    };
    fill_scd_name(&mut bf_ent.name, b".bf");
    entries.push(bf_ent);

    let mut ef_ent = ScdEntry {
        value: text_size,
        section: 1,
        scl: 0x65,
        len: 1,
        ..Default::default()
    };
    fill_scd_name(&mut ef_ent.name, b".ef");
    entries.push(ef_ent);

    let mut offset = 0u32;
    for (name, sect, idx) in [(b".text".as_slice(), 1i16, 0usize), (b".data".as_slice(), 2, 1), (b".bss".as_slice(), 3, 2)] {
        let size = obj.sections.get(idx).map(|s| s.size).unwrap_or(0);
        let mut ent = ScdEntry {
            value: offset,
            section: sect,
            scl: 0x78,
            len: 1,
            tag: size,
            ..Default::default()
        };
        fill_scd_name(&mut ent.name, name);
        entries.push(ent);
        offset = offset.wrapping_add(size);
    }

    let mut open_func_defs: Vec<usize> = Vec::new();
    for ev in &obj.scd_events {
        match ev {
            ScdEvent::Ln { line, location, section } => {
                // HAS 本体は .ln を .text のみに限定している。
                if *section == 1 {
                    lines.push((*location, *line));
                }
            }
            ScdEvent::Endef { name, attrib, value, section, scl, type_code, size, dim, is_long, .. } => {
                // HAS互換: enumメンバ（scl=16）は存在しないセクション(-2)へ補正する。
                let out_section = if *scl == 16 { -2 } else { *section };
                let mut ent = ScdEntry {
                    value: *value,
                    section: out_section,
                    type_code: *type_code,
                    scl: *scl,
                    len: if *is_long { 1 } else { 0 },
                    size: *size,
                    dim0: dim[0],
                    dim1: dim[1],
                    ..Default::default()
                };
                fill_scd_name(&mut ent.name, name);
                entries.push(ent);
                // HAS互換: 関数定義開始(0x21)は .scl -1 でサイズ確定させる。
                if *attrib == 0x21 {
                    open_func_defs.push(entries.len() - 1);
                }
            }
            ScdEvent::FuncEnd { location, section } => {
                if *section == 1 {
                    if let Some(idx) = open_func_defs.pop() {
                        let ent = &mut entries[idx];
                        ent.size = location.saturating_sub(ent.value);
                    }
                }
            }
            _ => {}
        }
    }
    // HAS 互換: `-g` のみ（`.file` 未使用）では先頭にダミー行番号エントリを持つ。
    if !obj.scd_enabled && lines.is_empty() {
        lines.push((2, 0));
    }
    // .ef の line count 相当を埋める（近似: 最大行番号）
    if let Some(ef) = entries.iter_mut().find(|e| &e.name[..3] == b".ef") {
        ef.size = lines.iter().map(|(_, l)| *l as u32).max().unwrap_or(0);
    }

    let line_len = (lines.len() as u32) * 6;
    let scd_len = (entries.len() as u32) * 36;
    let exname_len = exname_data.len() as u32;
    out.extend_from_slice(&line_len.to_be_bytes());
    out.extend_from_slice(&scd_len.to_be_bytes());
    out.extend_from_slice(&exname_len.to_be_bytes());

    for (loc, line) in lines {
        out.extend_from_slice(&loc.to_be_bytes());
        out.extend_from_slice(&line.to_be_bytes());
    }
    for ent in &entries {
        push_scd_entry(out, ent);
    }
    out.extend_from_slice(&exname_data);
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

    // ---- SCD デバッグ拡張部（HAS 互換: 0000 の後に続く）----
    write_scd_footer(&mut out, obj);

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
