mod common;
use common::*;
use std::io::Write;
use tempfile::{Builder, NamedTempFile};

/// -p オプションでPRNファイルが生成される
#[test]
fn test_prn_list_file() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tmove.b\td0,d1\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    // PRNファイル用の一時ファイルパス
    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    // PRNファイルが存在して内容が正しいか確認
    let prn_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn file");
    let prn_str = String::from_utf8_lossy(&prn_content);

    // 行番号1と2が含まれていること
    assert!(prn_str.contains("    1 "), "line 1 in PRN");
    assert!(prn_str.contains("    2 "), "line 2 in PRN");
    // アドレス 00000000 が含まれていること
    assert!(prn_str.contains("00000000"), "address in PRN");
    // コードバイトが含まれていること
    assert!(prn_str.contains("1200"), "move.b d0,d1 bytes in PRN");
    assert!(prn_str.contains("4E71"), "nop bytes in PRN");
}

/// -g オプションで $B204 レコードが出力される（.align 未使用でも出力）。
#[test]
fn test_g_option_emits_b204_record() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let found = result.obj_bytes.windows(2).any(|w| w == [0xB2, 0x04]);
    assert!(found, "B204 record should exist when -g is enabled");
}

/// HAS互換: `-g` のみ（`.file` 未使用）ではダミー + 自動行番号の2件を持つ。
#[test]
fn test_g_only_emits_default_scd_line_entry() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;
    let end_pos = (0..bytes.len().saturating_sub(14))
        .find(|&i| {
            if bytes[i] != 0x00 || bytes[i + 1] != 0x00 {
                return false;
            }
            let p = i + 2;
            let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
            let scd_len = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize;
            let exname_len = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]) as usize;
            p + 12 + line_len + scd_len + exname_len == bytes.len()
        })
        .expect("0000 terminator");
    let p = end_pos + 2;
    let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
    assert_eq!(line_len, 12);
    let q = p + 12;
    let loc0 = u32::from_be_bytes([bytes[q], bytes[q + 1], bytes[q + 2], bytes[q + 3]]);
    let line0 = u16::from_be_bytes([bytes[q + 4], bytes[q + 5]]);
    let loc1 = u32::from_be_bytes([bytes[q + 6], bytes[q + 7], bytes[q + 8], bytes[q + 9]]);
    let line1 = u16::from_be_bytes([bytes[q + 10], bytes[q + 11]]);
    assert_eq!(loc0, 2);
    assert_eq!(line0, 0);
    assert_eq!(loc1, 0);
    assert_eq!(line1, 1);
}

/// SCD有効時（`.file` モード）に `.ln` は行番号を保持し、2番目オペランド式も受理する。
#[test]
fn test_scd_ln_alias_updates_line_state() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.ln\t123,*\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let _ = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert_eq!(ctx.scd_ln, 123);
}

/// HAS互換: `.ln` 行番号は下位16bitへ丸めて保持する。
#[test]
fn test_scd_ln_wraps_to_u16() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.ln\t70000,*\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let _ = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert_eq!(ctx.scd_ln, 70000u32 as u16);
}

/// SCD有効時（`.file` モード）の `.dim` は定数4要素までを受理して一時バッファへ反映する。
#[test]
fn test_scd_dim_updates_temp_buffer() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.def\tfoo\n\t.dim\t1,2,3,4\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let _ = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert_eq!(ctx.scd_temp.dim, [1, 2, 3, 4]);
    assert!(ctx.scd_temp.is_long);
}

/// HAS互換: `.line` は値域チェックせず下位16bitのみ保持する。
#[test]
fn test_scd_line_wraps_to_u16_in_temp_size() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.def\tfoo\n\t.line\t70000\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let _ = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert!(ctx.scd_temp.is_long);
    assert_eq!(ctx.scd_temp.size, (70000u32 as u16) as u32);
}

/// SCD有効時（`.file` モード）の `.scl` は範囲外値を拒否する。
#[test]
fn test_scd_scl_rejects_out_of_range() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.scl\t256\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    match rhas::pass::assemble(&mut ctx, &mut reporter) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

/// HAS互換: `-g` 指定時は SCD 疑似命令を無視する。
#[test]
fn test_scd_directives_are_ignored_without_g() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.scl\t9999\n\t.dim\tA,B,C,D,E\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let _ = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
}

/// HAS互換: SCDフッタの `.file` 名は入力ソースファイル名を使う。
#[test]
fn test_scd_footer_uses_input_source_filename() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let expected_file = f
        .path()
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .into_owned()
        .into_bytes();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert_eq!(ctx.scd_file, b"".to_vec(), "directive should be ignored in -g mode");
    assert_eq!(result.obj.scd_file, expected_file, "footer name should be input source filename");
}

/// `-g` + `.file` 指定時でも、B204文字列は入力ソースファイル名を使う。
#[test]
fn test_scd_file_does_not_affect_b204_filename() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let expected_file = f
        .path()
        .file_name()
        .expect("filename")
        .to_string_lossy()
        .into_owned();
    let expected_pat = format!("*{}*", expected_file);

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let pat = expected_pat.as_bytes();
    assert!(
        result.obj_bytes.windows(pat.len()).any(|w| w == pat),
        "B204 payload should contain input source filename"
    );
    assert!(
        !result.obj_bytes.windows(b"*main.c*".len()).any(|w| w == b"*main.c*"),
        "B204 payload should not be replaced by .file name"
    );
}

/// SCD疑似命令は Pass1 で専用 TempRecord に変換される。
#[test]
fn test_scd_records_are_emitted_in_pass1() {
    let records = pass1_records(
        b"\t.file\t\"main.c\"\n\t.ln\t12,*\n\t.def\tfoo\n\t.val\t.\n\t.tag\tbar\n\t.scl\t-1\n\t.endef\n",
        false,
    );
    assert!(records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdLn { line, .. } if *line == 12)));
    assert!(records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdVal { rpn }
        if matches!(rpn.as_slice(), [rhas::expr::RPNToken::Location, rhas::expr::RPNToken::End]))));
    assert!(records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdTag { name } if name.as_slice() == b"bar")));
    assert!(records.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdFuncEnd { .. }
    )));
    assert!(!records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdEndef { .. })));
}

/// SCD疑似命令は Pass3 で ObjectCode.scd_events に収集される。
#[test]
fn test_scd_events_are_collected_in_object() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.ln\t7,*\n\t.def\tfoo\n\t.val\t.\n\t.endef\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert!(result.obj.scd_events.iter().any(|e| matches!(e, rhas::object::ScdEvent::Ln { line, .. } if *line == 7)));
    assert!(result.obj.scd_events.iter().any(|e| matches!(e, rhas::object::ScdEvent::Val { .. })));
    assert!(result.obj.scd_events.iter().any(|e| matches!(
        e,
        rhas::object::ScdEvent::Endef { name, value, section, .. }
            if name.as_slice() == b"foo" && *value == 0 && *section == 1
    )));
}

/// `-g` 時は `$0000` 終端の後ろに SCD フッタ（長さ3つ）が続く。
#[test]
fn test_g_option_emits_scd_footer_after_terminator() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\t.ln\t7,*\n\t.def\tfoo\n\t.val\t.\n\t.endef\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;
    let end_pos = bytes
        .windows(2)
        .position(|w| w == [0x00, 0x00])
        .expect("0000 terminator");
    assert!(bytes.len() >= end_pos + 2 + 12, "SCD footer header must exist");
    let p = end_pos + 2;
    let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
    let scd_len = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]);
    assert!(line_len >= 6, "line table should have at least one entry");
    assert!(scd_len >= 36, "scd table should have at least one entry");
}

/// `-g` 時の SCD フッタには `.bf` / `.ef` エントリが含まれる。
#[test]
fn test_g_option_scd_footer_contains_bf_ef_entries() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.file\t\"main.c\"\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert!(result.obj_bytes.windows(4).any(|w| w == b".bf\0"));
    assert!(result.obj_bytes.windows(4).any(|w| w == b".ef\0"));
}

/// 14文字超の入力ソース名は SCD フッタの exname 領域へ出力される。
#[test]
fn test_g_option_scd_footer_emits_exname_for_long_source_filename() {
    let mut f = Builder::new()
        .prefix("verylongdebugname_source_")
        .suffix(".s")
        .tempfile()
        .expect("tempfile");
    f.write_all(b"\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;
    let end_pos = bytes
        .windows(2)
        .position(|w| w == [0x00, 0x00])
        .expect("0000 terminator");
    let p = end_pos + 2;
    let exname_len = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]);
    assert!(exname_len >= 20);
}

/// `.val` の定数式は Endef に section=-1 として保持される。
#[test]
fn test_scd_val_constant_is_preserved_in_endef_snapshot() {
    let records = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tfoo\n\t.val\t4\n\t.endef\n",
        false,
    );
    assert!(records.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, value, section, .. }
            if name.as_slice() == b"foo" && *value == 4 && *section == -1
    )));
}

/// HAS互換: `-g` だけでは SCD疑似命令は有効化されず、`.file` が必要。
#[test]
fn test_scd_directives_require_file_directive() {
    let records = pass1_records(b"\t.ln\t123,*\n\t.def\tfoo\n\t.endef\n", false);
    assert!(!records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdLn { .. })));
    assert!(!records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdEndef { .. })));
}

/// HAS互換: `.scl -1` 後の `.endef` は SCDエントリを生成しない。
#[test]
fn test_scd_scl_minus1_suppresses_endef_record() {
    let records = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tfoo\n\t.scl\t-1\n\t.endef\n",
        false,
    );
    assert!(records.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdFuncEnd { .. }
    )));
    assert!(!records.iter().any(|r| matches!(r, rhas::pass::temp::TempRecord::ScdEndef { .. })));
}

/// HAS互換: `.type` は 0x20/0x30 のときのみロングテーブル化する。
#[test]
fn test_scd_type_long_table_only_for_function_or_array() {
    let rec_short = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tfoo\n\t.type\t$0010\n\t.endef\n",
        false,
    );
    assert!(rec_short.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, is_long, .. }
            if name.as_slice() == b"foo" && !*is_long
    )));

    let rec_long = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tbar\n\t.type\t$0020\n\t.endef\n",
        false,
    );
    assert!(rec_long.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, is_long, .. }
            if name.as_slice() == b"bar" && *is_long
    )));
}

/// HAS互換: `.scl 16`（enumメンバ）の `.endef` は section=-2 で出力される。
#[test]
fn test_scd_enum_member_forces_section_minus2_in_footer() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\t.def\tenumv\n\t.val\t.\n\t.scl\t16\n\t.endef\n\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut found = false;
    for e in offsets {
        let name = &bytes[e..e + 8];
        if name.starts_with(b"enumv") {
            let section = i16::from_be_bytes([bytes[e + 12], bytes[e + 13]]);
            assert_eq!(section, -2);
            found = true;
            break;
        }
    }
    assert!(found, "enumv SCD entry should exist");
}

/// HAS互換: `.endef` で未指定 attrib は type/scl から補完される。
#[test]
fn test_scd_endef_derives_attrib_from_type_and_scl() {
    let function = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tfunc\n\t.type\t$20\n\t.endef\n",
        false,
    );
    assert!(function.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, attrib, is_long, .. }
            if name.as_slice() == b"func" && *attrib == 0x21 && *is_long
    )));

    let tag = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\ttag1\n\t.scl\t10\n\t.endef\n",
        false,
    );
    assert!(tag.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, attrib, is_long, .. }
            if name.as_slice() == b"tag1" && *attrib == 0x11 && *is_long
    )));

    let ext = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\textv\n\t.scl\t2\n\t.endef\n",
        false,
    );
    assert!(ext.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, attrib, .. }
            if name.as_slice() == b"extv" && *attrib == 0x50
    )));

    let local = pass1_records(
        b"\t.file\t\"main.c\"\n\t.def\tlocv\n\t.endef\n",
        false,
    );
    assert!(local.iter().any(|r| matches!(
        r,
        rhas::pass::temp::TempRecord::ScdEndef { name, attrib, .. }
            if name.as_slice() == b"locv" && *attrib == 0x30
    )));
}

/// HAS互換: `.scl -1` は直前の関数定義エントリの size を現在位置で確定する。
#[test]
fn test_scd_funcend_updates_function_size_in_footer() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\tfunc\n\
\t.val\t.\n\
\t.type\t$20\n\
\t.endef\n\
\tnop\n\
\tnop\n\
\t.scl\t-1\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut found = false;
    for e in offsets {
        let name = &bytes[e..e + 8];
        if name.starts_with(b"func") {
            let type_code = u16::from_be_bytes([bytes[e + 14], bytes[e + 15]]);
            let size = u32::from_be_bytes([bytes[e + 22], bytes[e + 23], bytes[e + 24], bytes[e + 25]]);
            if type_code == 0x0020 {
                assert_eq!(size, 4);
                found = true;
                break;
            }
        }
    }
    assert!(found, "function SCD entry should exist");
}

/// HAS互換: `.tag <name>` は Endef エントリの tag フィールドへ反映される。
#[test]
fn test_scd_tag_links_to_existing_tag_definition() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\tmytag\n\
\t.scl\t10\n\
\t.endef\n\
\t.def\tvar1\n\
\t.tag\tmytag\n\
\t.endef\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut mytag_idx = None;
    let mut var_tag = None;
    for (i, e) in offsets.iter().copied().enumerate() {
        let name = &bytes[e..e + 8];
        if name.starts_with(b"mytag") {
            mytag_idx = Some(i as u32);
        } else if name.starts_with(b"var1") {
            var_tag = Some(u32::from_be_bytes([bytes[e + 18], bytes[e + 19], bytes[e + 20], bytes[e + 21]]));
        }
    }
    assert_eq!(var_tag, mytag_idx);
}

/// HAS互換: 未解決 `.tag` が付いた `.endef` は SCD エントリを出力しない。
#[test]
fn test_scd_unresolved_tag_suppresses_endef_entry() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\tvar1\n\
\t.tag\tmissing\n\
\t.endef\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    assert!(
        !result.obj_bytes.windows(5).any(|w| w == b"var1\0"),
        "unresolved tag should suppress var1 SCD entry"
    );
}

/// HAS互換: 長いソース名の `.file` は14バイト領域末尾へ拡張子を寄せ、
/// 追記領域へ SCDFILENUM(=2) を書く。
#[test]
fn test_scd_file_entry_moves_short_extension_for_long_filename() {
    let mut f = Builder::new()
        .prefix("case_tag_missing_longname_")
        .suffix(".s")
        .tempfile()
        .expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut found = false;
    for e in offsets {
        if bytes[e..e + 8].starts_with(b".file") {
            let file14 = &bytes[e + 18..e + 32];
            assert_eq!(&file14[12..14], b".s");
            assert_eq!(&bytes[e + 32..e + 36], &[0x00, 0x00, 0x00, 0x02]);
            found = true;
            break;
        }
    }
    assert!(found, ".file entry should exist");
}

/// HAS互換: `.file` モードではファイル名が14文字までは exname を使わず、
/// 15文字以上で exname を使う。
#[test]
fn test_scd_file_mode_exname_boundary_14_vs_15() {
    // 14 chars: "1234567890abcd"
    let mut f14 = NamedTempFile::new().expect("tempfile");
    f14.write_all(
        b"\t.file\t\"1234567890abcd\"\n\
\tnop\n",
    )
    .expect("write");
    let path14 = f14.path().to_str().expect("path").as_bytes().to_vec();
    let opts14 = rhas::options::Options {
        source_file: Some(path14),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx14 = rhas::context::AssemblyContext::new(opts14);
    let mut reporter = rhas::error::BufferReporter::new(ctx14.effective_warn_level());
    let result14 = rhas::pass::assemble(&mut ctx14, &mut reporter).expect("assemble");
    let (p14, line14, scd14, ex14) = find_scd_footer(&result14.obj_bytes);
    assert_eq!(ex14, 0, "14-char .file should not use exname");
    let entries14 = scd_entry_offsets(&result14.obj_bytes, p14, line14, scd14);
    let file_ent14 = entries14
        .into_iter()
        .find(|&e| result14.obj_bytes[e..e + 8].starts_with(b".file"))
        .expect(".file entry");
    assert_eq!(
        &result14.obj_bytes[file_ent14 + 18..file_ent14 + 32],
        b"1234567890abcd"
    );

    // 15 chars: "1234567890abcde"
    let mut f15 = NamedTempFile::new().expect("tempfile");
    f15.write_all(
        b"\t.file\t\"1234567890abcde\"\n\
\tnop\n",
    )
    .expect("write");
    let path15 = f15.path().to_str().expect("path").as_bytes().to_vec();
    let opts15 = rhas::options::Options {
        source_file: Some(path15),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx15 = rhas::context::AssemblyContext::new(opts15);
    let mut reporter = rhas::error::BufferReporter::new(ctx15.effective_warn_level());
    let result15 = rhas::pass::assemble(&mut ctx15, &mut reporter).expect("assemble");
    let (p15, line15, scd15, ex15) = find_scd_footer(&result15.obj_bytes);
    assert!(ex15 >= 4, "15-char .file should use exname");
    let scd_base15 = p15 + 12 + line15;
    let scd_end15 = scd_base15 + scd15;
    let exname15 = &result15.obj_bytes[scd_end15..scd_end15 + ex15];
    assert_eq!(&exname15[0..2], &[0, 0]);
    assert!(
        exname15.windows(b"1234567890abcde".len()).any(|w| w == b"1234567890abcde"),
        "exname should contain the full .file name"
    );
}

/// HAS互換: `.bb` と `.eb` は .bb.next にチェインを形成する。
#[test]
fn test_scd_bb_eb_updates_bb_next_chain() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\t.bb\n\
\t.endef\n\
\t.def\t.eb\n\
\t.endef\n\
\t.nop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut bb_idx = None;
    let mut bb_next = None;
    let mut eb_idx = None;
    for (i, e) in offsets.iter().copied().enumerate() {
        let name = &bytes[e..e + 8];
        if name.starts_with(b".bb") {
            bb_idx = Some(i as u32);
            bb_next = Some(u32::from_be_bytes([bytes[e + 30], bytes[e + 31], bytes[e + 32], bytes[e + 33]]));
        } else if name.starts_with(b".eb") {
            eb_idx = Some(i as u32);
        }
    }
    assert!(bb_idx.is_some() && eb_idx.is_some());
    // HAS互換: next はレコード(18バイト単位)インデックス
    assert_eq!(bb_next, eb_idx.map(|v| Some(scd_next_record_after(bytes, &offsets, v as usize))).unwrap());
}

/// HAS互換: `.eb` / `.ef` が単独で現れてもチェイン書き戻しは行わず、
/// 既存エントリを壊さない。
#[test]
fn test_scd_orphan_eb_ef_keep_next_unchanged() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\t.eb\n\
\t.endef\n\
\t.def\t.ef\n\
\t.endef\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);
    let mut eb_next = None;
    let mut ef_next = None;
    for e in offsets {
        let name = &bytes[e..e + 8];
        if name.starts_with(b".eb") {
            eb_next = Some(u32::from_be_bytes([bytes[e + 30], bytes[e + 31], bytes[e + 32], bytes[e + 33]]));
        } else if name.starts_with(b".ef") {
            ef_next = Some(u32::from_be_bytes([bytes[e + 30], bytes[e + 31], bytes[e + 32], bytes[e + 33]]));
        }
    }
    assert_eq!(eb_next, Some(0));
    assert_eq!(ef_next, Some(0));
}

/// HAS互換: `.tag` は最後に指定された名前を採用し、未解決タグ指定の後でも
/// 解決可能な `.tag` が来れば Endef は出力される。
#[test]
fn test_scd_tag_last_wins_after_unresolved_tag() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\ttagok\n\
\t.scl\t10\n\
\t.endef\n\
\t.def\tvar1\n\
\t.tag\tmissing\n\
\t.tag\ttagok\n\
\t.endef\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);
    let mut tagok_idx = None;
    let mut var_tag = None;
    for (i, e) in offsets.iter().copied().enumerate() {
        let name = &bytes[e..e + 8];
        if name.starts_with(b"tagok") {
            tagok_idx = Some(i as u32);
        } else if name.starts_with(b"var1") {
            var_tag = Some(u32::from_be_bytes([bytes[e + 18], bytes[e + 19], bytes[e + 20], bytes[e + 21]]));
        }
    }
    // HAS互換: tag はレコード(18バイト単位)インデックス
    assert_eq!(var_tag, tagok_idx.map(|v| scd_record_index(bytes, &offsets, v as usize)));
}

/// HAS互換: `.val` は Pass3 で再評価され、forward 参照でも Endef 値へ反映される。
#[test]
fn test_scd_val_forward_symbol_is_resolved_in_footer() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"\t.file\t\"main.c\"\n\
\t.def\tfwdv\n\
\t.val\ttarget\n\
\t.endef\n\
\tnop\n\
target:\n\
\tnop\n",
    )
    .expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        make_sym_deb: false,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    let bytes = &result.obj_bytes;

    let (p, line_len, scd_len, _) = find_scd_footer(bytes);
    let offsets = scd_entry_offsets(bytes, p, line_len, scd_len);

    let mut found = false;
    for e in offsets {
        let name = &bytes[e..e + 8];
        if name.starts_with(b"fwdv") {
            let value = u32::from_be_bytes([bytes[e + 8], bytes[e + 9], bytes[e + 10], bytes[e + 11]]);
            let section = i16::from_be_bytes([bytes[e + 12], bytes[e + 13]]);
            assert_eq!(value, 2);
            assert_eq!(section, 1);
            found = true;
            break;
        }
    }
    assert!(found, "forward .val SCD entry should exist");
}

/// `.nlist` 区間は PRN に出力されず、`.list` で再開される。
#[test]
fn test_prn_nlist_and_list() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.nlist\n\tmove.b\td0,d1\n\t.list\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn file");
    let prn_str = String::from_utf8_lossy(&prn_content);

    assert!(!prn_str.contains("1200"), "nlist section should be hidden");
    assert!(prn_str.contains("4E71"), "list section should be visible");
}

/// `.lall` 指定時はマクロ展開行が PRN に `*` マーク付きで表示される。
#[test]
fn test_prn_lall_shows_macro_expansion_lines() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(
        b"m\t.macro\n\tnop\n\t.endm\n\t.lall\n\tm\n"
    ).expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn file");
    let prn_str = String::from_utf8_lossy(&prn_content);

    assert!(prn_str.contains("*4E71"), "macro expansion line should be marked with '*'");
}

/// `.width` の値が PRN 1行の表示幅に反映される。
#[test]
fn test_prn_width_directive_limits_line_width() {
    let mut src = Vec::<u8>::new();
    src.extend_from_slice(b"\t.width\t80\n\tnop\t;");
    src.extend(std::iter::repeat_n(b'A', 160));
    src.extend_from_slice(b"\n");

    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(&src).expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn file");
    let prn_str = String::from_utf8_lossy(&prn_content);

    let max_len = prn_str.lines().map(|l| l.len()).max().unwrap_or(0);
    assert!(max_len <= 80, "PRN line should be clipped to width 80, got {}", max_len);
}

/// `.title/.subttl` で指定した文字列が PRN ヘッダに反映される。
#[test]
fn test_prn_title_and_subttl_are_reflected() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.title\t\"MainTitle\"\n\t.subttl\t\"PartA\"\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn file");
    let prn_str = String::from_utf8_lossy(&prn_content);

    assert!(prn_str.contains("MainTitle"), "title should appear in PRN");
    assert!(prn_str.contains("PartA"), "subttl should appear in PRN");
}

/// `.page` が PRN にフォームフィードを出力する（`-f0` では抑制）。
#[test]
fn test_prn_page_emits_formfeed_unless_disabled() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tnop\n\t.page\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file_a = NamedTempFile::new().expect("prn tempfile");
    let prn_path_a = prn_file_a.path().to_str().expect("path").as_bytes().to_vec();
    let mut opts_a = rhas::options::Options {
        source_file: Some(src_path.clone()),
        make_prn: true,
        prn_file: Some(prn_path_a.clone()),
        ..Default::default()
    };
    opts_a.prn_no_page_ff = false;
    let mut ctx_a = rhas::context::AssemblyContext::new(opts_a);
    let mut reporter = rhas::error::BufferReporter::new(ctx_a.effective_warn_level());
    rhas::pass::assemble(&mut ctx_a, &mut reporter).expect("assemble a");
    let prn_a = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path_a).unwrap()
    )).expect("read prn a");
    assert!(prn_a.contains(&0x0C), "formfeed should be emitted for .page");

    let prn_file_b = NamedTempFile::new().expect("prn tempfile");
    let prn_path_b = prn_file_b.path().to_str().expect("path").as_bytes().to_vec();
    let mut opts_b = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path_b.clone()),
        ..Default::default()
    };
    opts_b.prn_no_page_ff = true;
    let mut ctx_b = rhas::context::AssemblyContext::new(opts_b);
    let mut reporter = rhas::error::BufferReporter::new(ctx_b.effective_warn_level());
    rhas::pass::assemble(&mut ctx_b, &mut reporter).expect("assemble b");
    let prn_b = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path_b).unwrap()
    )).expect("read prn b");
    assert!(!prn_b.contains(&0x0C), "formfeed should be suppressed when no_page_ff");
}

/// `.page <expr>` は改ページせず、PRNページ行数設定を更新する。
#[test]
fn test_prn_page_with_expr_sets_page_lines_without_formfeed() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tnop\n\t.page\t60\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    assert_eq!(ctx.opts.prn_page_lines, 60);

    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(!prn.contains(&0x0C), "formfeed should not be emitted for .page <expr>");
}

/// `prn_page_lines` に達すると自動でフォームフィード改ページされる。
#[test]
fn test_prn_auto_page_break_by_line_limit() {
    let mut src = Vec::<u8>::new();
    for _ in 0..12 {
        src.extend_from_slice(b"\tnop\n");
    }

    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(&src).expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let mut opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    opts.prn_page_lines = 10;
    opts.prn_no_page_ff = false;

    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(prn.contains(&0x0C), "auto page break should emit formfeed");
}

/// `.page -1` は自動改ページを無効化する。
#[test]
fn test_prn_page_minus1_disables_auto_page_break() {
    let mut src = Vec::<u8>::new();
    src.extend_from_slice(b"\t.page\t-1\n");
    for _ in 0..20 {
        src.extend_from_slice(b"\tnop\n");
    }

    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(&src).expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let mut opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    opts.prn_page_lines = 10;
    opts.prn_no_page_ff = false;

    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    assert_eq!(ctx.opts.prn_page_lines, u16::MAX, ".page -1 should disable auto page break");
    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(!prn.contains(&0x0C), "no formfeed expected when auto page break disabled");
}

/// `.page +` は明示改ページとしてフォームフィードを出力する。
#[test]
fn test_prn_page_plus_emits_formfeed() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\tnop\n\t.page\t+\n\tnop\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let mut opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    opts.prn_no_page_ff = false;

    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(prn.contains(&0x0C), "formfeed should be emitted for .page +");
}

/// `prn_no_page_ff` が true のとき、明示/自動いずれの改ページも抑制される。
#[test]
fn test_prn_no_page_ff_disables_all_formfeed() {
    let mut src = Vec::<u8>::new();
    src.extend_from_slice(b"\t.page\t+\n");
    for _ in 0..20 {
        src.extend_from_slice(b"\tnop\n");
    }

    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(&src).expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let prn_file = NamedTempFile::new().expect("prn tempfile");
    let prn_path = prn_file.path().to_str().expect("path").as_bytes().to_vec();

    let mut opts = rhas::options::Options {
        source_file: Some(src_path),
        make_prn: true,
        prn_file: Some(prn_path.clone()),
        ..Default::default()
    };
    opts.prn_no_page_ff = true;
    opts.prn_page_lines = 10;

    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(!prn.contains(&0x0C), "no formfeed should be emitted when prn_no_page_ff=true");
}
