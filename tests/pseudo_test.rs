mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

/// .equ シンボル参照
#[test]
fn test_equ_symbol() {
    let src = b"\
CONST\t.equ\t42\n\
\tmoveq\t#CONST,d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // moveq #42,d0 = 0x702A (2 bytes)
    assert_eq!(&text.bytes, &[0x70, 42], "moveq #42,d0");
}

/// `*` を使う .equ は行頭ロケーションで評価される。
#[test]
fn test_equ_location_counter_uses_line_top() {
    let src = b"\
base:\n\
\tnop\n\
ofs\t.equ\t(*)-base\n\
\taddq.l\t#ofs,(sp)\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // ofs = 2 なので addq.l #2,(sp) = 0x5497
    assert_eq!(text.bytes, [0x4E, 0x71, 0x54, 0x97]);
}

/// .dc.b / .dc.w / .dc.l データ定義
#[test]
fn test_dc_directives() {
    let src = b"\
\t.dc.b\t1,2,3\n\
\t.dc.w\t0x0100\n\
\t.dc.l\t0x01020304\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    assert_eq!(
        text.bytes,
        [1, 2, 3, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04]
    );
}

/// ラベル差分を含む .dc は Pass3 で最終ラベル値を使って評価される。
#[test]
fn test_dc_label_diff_recomputed_after_pass2() {
    let src = b"\
tbl:\n\
\t.dc.w\tlbl-tbl\n\
\tbra\tend\n\
\tnop\n\
lbl:\n\
\tnop\n\
end:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x00, 0x06, 0x60, 0x04, 0x4E, 0x71, 0x4E, 0x71, 0x4E, 0x71]);
}

/// .ds.w はテキストセクションで予約レコード ($3000) を生成する（実バイトなし）
#[test]
fn test_ds_directive() {
    let src = b"\t.ds.w\t3\n";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // .ds はテキストセクションでも $3000 予約レコードを使う（HAS互換）
    // sect_bytes には実バイトは入らないが、size はカウントされる
    assert_eq!(text.size, 6, ".ds.w 3 = 6 bytes reserved");
    assert!(text.bytes.is_empty(), ".ds.w in text: no actual bytes in sect_bytes");
}

/// .comm/.rcomm/.rlcomm は B2FE/B2FD/B2FC 外部シンボルとして出力される。
#[test]
fn test_common_symbol_directives_emit_ext_symbols() {
    let src = b"\
\t.comm\tcbuf,16\n\
\t.rcomm\trbuf,8\n\
\t.rlcomm\tlbuf,4\n\
";
    let result = assemble_src(src);

    let cbuf = result.obj.ext_syms.iter().find(|s| s.name.as_slice() == b"cbuf").expect("cbuf");
    assert_eq!(cbuf.kind, rhas::object::sym_kind::COMM);
    assert_eq!(cbuf.value, 16);

    let rbuf = result.obj.ext_syms.iter().find(|s| s.name.as_slice() == b"rbuf").expect("rbuf");
    assert_eq!(rbuf.kind, rhas::object::sym_kind::R_COMM);
    assert_eq!(rbuf.value, 8);

    let lbuf = result.obj.ext_syms.iter().find(|s| s.name.as_slice() == b"lbuf").expect("lbuf");
    assert_eq!(lbuf.kind, rhas::object::sym_kind::RL_COMM);
    assert_eq!(lbuf.value, 4);
}

/// .comm サイズは正の定数のみ許可される。
#[test]
fn test_comm_rejects_non_positive_size() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.comm\tbuf,0\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    match rhas::pass::assemble(&mut ctx, &mut reporter) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => {
            assert!(n >= 1);
            assert!(reporter.errors.iter().any(|e| e.code == rhas::error::ErrorCode::IlValue));
        }
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

/// .comm は .sym 出力で UNDEF ではなくサイズ付きシンボルとして表示される。
#[test]
fn test_comm_symbol_is_visible_in_sym_file() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.comm\tbuf,16\n").expect("write");
    let src_path = f.path().to_str().expect("path").as_bytes().to_vec();

    let sym_file = NamedTempFile::new().expect("sym tempfile");
    let sym_path = sym_file.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(src_path),
        make_sym: true,
        sym_file: Some(sym_path.clone()),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");

    let sym_content = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&sym_path).unwrap()
    )).expect("read sym file");
    let sym_str = String::from_utf8_lossy(&sym_content);
    assert!(sym_str.contains("buf"), "symbol name should be present");
    assert!(sym_str.contains("COMM"), "COMM type should be present");
    assert!(sym_str.contains("00000010"), "size value should be present");
}

/// .offsym <expr> は .offset <expr> と同様に絶対セクションのロケーションを設定する。
#[test]
fn test_offsym_without_symbol_behaves_like_offset() {
    let src = b"\
\t.offsym\t16\n\
A:\n\
\t.text\n\
\tmoveq\t#A,d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x70, 0x10]);
}

/// .offsym <expr>,<sym> はシンボルへ初期値を与え、絶対値として参照できる。
#[test]
fn test_offsym_with_symbol_sets_symbol_value() {
    let src = b"\
\t.offsym\t32,BASE\n\
\t.text\n\
\tmoveq\t#BASE,d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x70, 0x20]);
}

/// `.offsym <expr>,<sym>` 中は `.even/.quad/.align` を許可しない。
#[test]
fn test_offsym_with_symbol_rejects_alignment_directives() {
    for src in [
        b"\t.offsym\t0,BASE\n\t.even\n".as_slice(),
        b"\t.offsym\t0,BASE\n\t.quad\n".as_slice(),
        b"\t.offsym\t0,BASE\n\t.align\t4\n".as_slice(),
    ] {
        let mut f = NamedTempFile::new().expect("tempfile");
        f.write_all(src).expect("write");
        let path = f.path().to_str().expect("path").as_bytes().to_vec();
        let opts = rhas::options::Options {
            source_file: Some(path),
            ..Default::default()
        };
        let mut ctx = rhas::context::AssemblyContext::new(opts);
        let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
        match rhas::pass::assemble(&mut ctx, &mut reporter) {
            Err(rhas::pass::AssembleError::HasErrors(n)) => {
                assert!(n >= 1);
                assert!(reporter.errors.iter().any(|e| e.code == rhas::error::ErrorCode::OffsymAlign));
            }
            Err(other) => panic!("unexpected error: {:?}", other),
            Ok(_) => panic!("assemble should fail"),
        }
    }
}

/// `.offsym` の上書きはデフォルトで警告、`ow_offsym` 有効時はエラー。
#[test]
fn test_offsym_overwrite_warning_and_error_mode() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"X\t.equ\t1\n\t.offsym\t2,X\n\t.text\n\tmoveq\t#X,d0\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts_warn = rhas::options::Options {
        source_file: Some(path.clone()),
        ..Default::default()
    };
    let mut ctx_warn = rhas::context::AssemblyContext::new(opts_warn);
    let mut reporter_warn = rhas::error::BufferReporter::new(ctx_warn.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx_warn, &mut reporter_warn).expect("assemble warn mode");
    assert!(result.num_warnings >= 1, "overwrite should emit warning in default mode");
    assert!(reporter_warn.warnings.iter().any(|w| w.code == rhas::error::warn::REDEF_OFFSYM));

    let mut opts_err = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    opts_err.ow_offsym = true;
    let mut ctx_err = rhas::context::AssemblyContext::new(opts_err);
    let mut reporter_err = rhas::error::BufferReporter::new(ctx_err.effective_warn_level());
    match rhas::pass::assemble(&mut ctx_err, &mut reporter_err) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => {
            assert!(n >= 1);
            assert!(reporter_err.errors.iter().any(|e| e.code == rhas::error::ErrorCode::RedefOffsym));
        }
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail when ow_offsym is enabled"),
    }
}

/// `.request` は `$E001` レコードとして出力される。
#[test]
fn test_request_emits_e001_record() {
    let result = assemble_src(b"\t.request\t\"libfoo.r\"\n\tnop\n");
    assert_eq!(result.obj.request_files, vec![b"libfoo.r".to_vec()]);

    let found = result.obj_bytes.windows(2).any(|w| w == [0xE0, 0x01]);
    assert!(found, "E001 record should exist when .request is used");
}
