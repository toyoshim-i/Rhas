//! End-to-end integration tests
//!
//! アセンブラの 3 パスパイプライン全体を検証する。

use std::io::Write;
use std::path::PathBuf;
use tempfile::{Builder, NamedTempFile};

// ─── ヘルパー ────────────────────────────────────────────────────────────────

/// ソーステキストからオブジェクトコードを生成して HLK バイト列を返す。
fn assemble_src(src: &[u8]) -> rhas::pass::AssembleResult {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    rhas::pass::assemble(&mut ctx).expect("assemble")
}

/// -c4 相当の拡張最適化を有効にしてアセンブルする。
fn assemble_src_c4(src: &[u8]) -> rhas::pass::AssembleResult {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        opt_clr: true,
        opt_movea: true,
        opt_adda_suba: true,
        opt_cmpa: true,
        opt_lea: true,
        opt_asl: true,
        opt_cmp0: true,
        opt_move0: true,
        opt_cmpi0: true,
        opt_sub_addi0: true,
        opt_bsr: true,
        opt_jmp_jsr: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    rhas::pass::assemble(&mut ctx).expect("assemble")
}

/// コンテキストを返しつつアセンブルする（pass遷移確認用）。
fn assemble_with_ctx(src: &[u8]) -> (rhas::pass::AssembleResult, rhas::context::AssemblyContext) {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
    (result, ctx)
}

/// ソーステキストを Pass1 のみ実行し、生成された TempRecord を返す。
fn pass1_records(src: &[u8], make_sym_deb: bool) -> Vec<rhas::pass::temp::TempRecord> {
    let buf = rhas::source::SourceBuf::from_bytes(src.to_vec(), PathBuf::from("inline.s"));
    let mut source = rhas::source::SourceStack::new(buf, vec![]);
    let opts = rhas::options::Options {
        make_sym_deb,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut sym = rhas::symbol::SymbolTable::new(false);
    rhas::pass::pass1::pass1(&mut source, &mut ctx, &mut sym)
}

fn find_scd_footer(bytes: &[u8]) -> (usize, usize, usize, usize) {
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
    let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
    let scd_len = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize;
    let exname_len = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]) as usize;
    (p, line_len, scd_len, exname_len)
}

fn scd_entry_offsets(bytes: &[u8], p: usize, line_len: usize, scd_len: usize) -> Vec<usize> {
    let scd_base = p + 12 + line_len;
    let mut out = Vec::new();
    let mut cur = scd_base;
    let end = scd_base + scd_len;
    while cur < end {
        out.push(cur);
        let len = bytes[cur + 17] as usize;
        cur += (len + 1) * 18;
    }
    assert_eq!(cur, end, "SCD entry stream length mismatch");
    out
}

// ─── MS1: 最小アセンブル ─────────────────────────────────────────────────────

/// move.b d0,d1 が 0x1200 にエンコードされ、正しい HLK ファイルが出力される。
#[test]
fn test_ms1_move_b_d0_d1() {
    let result = assemble_src(b"\tmove.b\td0,d1\n");

    // text セクションに 0x12 0x00 が入っていること
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    assert_eq!(text.bytes, [0x12, 0x00], "move.b d0,d1 should encode to 0x12 0x00");

    // HLK バイナリの先頭が D0 00 であること
    assert_eq!(&result.obj_bytes[0..2], &[0xD0, 0x00], "HLK header");

    // 終端 00 00 であること
    let len = result.obj_bytes.len();
    assert_eq!(&result.obj_bytes[len-2..], &[0x00, 0x00], "HLK terminator");
}

#[test]
fn test_assemble_sets_final_pass_to_pass3() {
    let (_result, ctx) = assemble_with_ctx(b"\tnop\n");
    assert_eq!(ctx.pass, rhas::context::AsmPass::Pass3);
}

/// 複数命令のアセンブル
#[test]
fn test_multiple_instructions() {
    let src = b"\
\tmove.b\td0,d1\n\
\tadd.w\td1,d2\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // move.b d0,d1 = 0x1200 (2 bytes)
    // add.w  d1,d2 = 0xD441 (2 bytes)
    assert_eq!(text.bytes.len(), 4, "two instructions = 4 bytes");
    assert_eq!(&text.bytes[0..2], &[0x12, 0x00], "move.b d0,d1");
    assert_eq!(&text.bytes[2..4], &[0xD4, 0x41], "add.w d1,d2");
}

/// ラベルとブランチ
#[test]
fn test_label_and_bra() {
    let src = b"\
loop:\n\
\tnop\n\
\tbra\tloop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // nop = 2 bytes (0x4E71)
    // bra loop: offset = 0 - (2+2) = -4 → fits in .s range → pass2 optimizes to bra.s (2 bytes)
    assert_eq!(text.bytes.len(), 4, "nop + bra.s = 4 bytes");
    assert_eq!(&text.bytes[0..2], &[0x4E, 0x71], "nop");
    // BRA.S: 0x60xx where xx = displacement byte (-4 = 0xFC)
    assert_eq!(&text.bytes[2..4], &[0x60, 0xFC], "bra.s loop offset=-4");
}

/// 直後ラベルへの BRA は pass2 でサプレスされる（HAS互換）
#[test]
fn test_bra_to_next_is_suppressed() {
    let src = b"\
\tbra\tnext\n\
next:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x4E, 0x71], "bra next should be removed");
}

/// 数値ローカルラベル `1f` は直近の前方 `1:` に解決される。
#[test]
fn test_numeric_local_label_forward() {
    let src = b"\
\tbne\t1f\n\
\tnop\n\
1:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x66, 0x02, 0x4E, 0x71, 0x4E, 0x71]);
}

/// 数値ローカルラベル `1b` は直近の後方 `1:` に解決される。
#[test]
fn test_numeric_local_label_backward() {
    let src = b"\
1:\n\
\tnop\n\
\tbne\t1b\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(text.bytes, [0x4E, 0x71, 0x66, 0xFC]);
}

/// 数値ローカルラベル展開は16進リテラル `$2b` を誤変換しない。
#[test]
fn test_numeric_local_label_does_not_touch_hex_literal() {
    let src = b"\
\tmoveq\t#$2b,d0\n\
\tbne\t1f\n\
\tnop\n\
1:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // moveq #$2b,d0 ; bne.s +2 ; nop ; nop
    assert_eq!(text.bytes, [0x70, 0x2B, 0x66, 0x02, 0x4E, 0x71, 0x4E, 0x71]);
}

/// Pass2 は DeferredInsn のサイズ変化をラベル値へ反映する。
/// 反映漏れがあると bra target のオフセットが +4 になってしまう。
#[test]
fn test_pass2_updates_labels_after_deferred_size_change() {
    let src = b"\
\tbra\ttarget\n\
\tmove.w\t(target-target,a0),d0\n\
target:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // 正しいオフセットは +2 (pc+2 -> target=0x0004)。
    assert_eq!(text.bytes, [0x60, 0x02, 0x30, 0x10, 0x4E, 0x71]);
}

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

/// セクション切り替え
#[test]
fn test_section_switch() {
    let src = b"\
\t.text\n\
\tmove.b\td0,d1\n\
\t.data\n\
\t.dc.w\t0x1234\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1);
    let data = result.obj.sections.iter().find(|s| s.id == 2);
    assert!(text.is_some(), "text section");
    assert!(data.is_some(), "data section");
    assert_eq!(text.unwrap().bytes, [0x12, 0x00]);
    assert_eq!(data.unwrap().bytes, [0x12, 0x34]);
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

/// Pass1 の DeferToLinker 再エンコードで動的 .equ 値を早期固定しない。
#[test]
fn test_addq_immediate_from_dynamic_equ_not_frozen_in_pass1() {
    let src = b"\
base:\n\
\tbne\ttarget\n\
ofs\t.equ\t(*)-base\n\
\taddq.l\t#ofs,(sp)\n\
target:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // bne.w -> bne.s に短縮されるため ofs は 4 ではなく 2。
    // bne.s target ; addq.l #2,(sp) ; nop
    assert_eq!(text.bytes, [0x66, 0x02, 0x54, 0x97, 0x4E, 0x71]);
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
    match rhas::pass::assemble(&mut ctx) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
        match rhas::pass::assemble(&mut ctx) {
            Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
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
    let result = rhas::pass::assemble(&mut ctx_warn).expect("assemble warn mode");
    assert!(result.num_warnings >= 1, "overwrite should emit warning in default mode");

    let mut opts_err = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    opts_err.ow_offsym = true;
    let mut ctx_err = rhas::context::AssemblyContext::new(opts_err);
    match rhas::pass::assemble(&mut ctx_err) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail when ow_offsym is enabled"),
    }
}

/// `.fpid` は 0..7 を受け付け、負値では CFPP を無効化する。
#[test]
fn test_fpid_sets_id_and_can_disable_fpu() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.fpid\t3\n\t.fpid\t-1\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
    assert_eq!(ctx.fpid, 3);
    assert_eq!(ctx.cpu_type & rhas::options::cpu::CFPP, 0, "negative .fpid should disable CFPP");
}

/// `.fpid` は 0..7 以外を拒否する。
#[test]
fn test_fpid_rejects_out_of_range() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.fpid\t8\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    match rhas::pass::assemble(&mut ctx) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

/// FPU コア命令のエンコード（FNOP/FMOVE/FADD/FCMP/FTST/FMOVECR/FSAVE/FRESTORE）。
#[test]
fn test_fpu_core_instruction_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfnop\n\
\tfmove.x\tfp0,fp1\n\
\tfadd.l\td0,fp1\n\
\tfcmp.x\tfp2,fp1\n\
\tftst\t(a0)\n\
\tfmovecr\t#1,fp2\n\
\tfsave\t(a0)\n\
\tfrestore\t(a0)\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF6, 0x80, 0x00, 0x00, // fnop
            0xF6, 0x00, 0x00, 0x80, // fmove.x fp0,fp1
            0xF6, 0x00, 0x40, 0xA2, // fadd.l d0,fp1
            0xF6, 0x00, 0x08, 0xB8, // fcmp.x fp2,fp1
            0xF6, 0x10, 0x48, 0x3A, // ftst (a0) (default .x)
            0xF6, 0x00, 0x5D, 0x01, // fmovecr #1,fp2
            0xF7, 0x10,             // fsave (a0)
            0xF7, 0x50,             // frestore (a0)
        ]
    );
}

/// FMOVE のデフォルトサイズはメモリ経由で .x になる。
#[test]
fn test_fmove_default_size_is_extend_for_memory_forms() {
    let src = b"\
\t.68040\n\
\tfmove\t(a0),fp1\n\
\tfmove\tfp1,(a0)\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF2, 0x10, 0x48, 0x80,
            0xF2, 0x10, 0x68, 0x80,
        ]
    );
}

/// FMOVEM (control registers) のエンコード。
#[test]
fn test_fmovem_control_register_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem\tfpcr,(a0)\n\
\tfmovem\tfpsr,(a0)\n\
\tfmovem\tfpiar,(a0)\n\
\tfmovem\t(a0),fpcr\n\
\tfmovem\t(a0),fpsr\n\
\tfmovem\t(a0),fpiar\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF6, 0x10, 0xB0, 0x00,
            0xF6, 0x10, 0xA8, 0x00,
            0xF6, 0x10, 0xA4, 0x00,
            0xF6, 0x10, 0x90, 0x00,
            0xF6, 0x10, 0x88, 0x00,
            0xF6, 0x10, 0x84, 0x00,
        ]
    );
}

/// FMOVEM FPn レジスタリスト（静的リスト）のエンコード。
#[test]
fn test_fmovem_fpreg_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.x\tfp0/fp1,(a0)\n\
\tfmovem.x\t(a0),fp0/fp1\n\
\tfmovem.x\tfp0/fp1,-(a0)\n\
\tfmovem.x\t(a0)+,fp0/fp1\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xF0, 0xC0, // fmovem.x fp0/fp1,(a0)
            0xF6, 0x10, 0xD0, 0xC0, // fmovem.x (a0),fp0/fp1
            0xF6, 0x20, 0xE0, 0x03, // fmovem.x fp0/fp1,-(a0)
            0xF6, 0x18, 0xD0, 0xC0, // fmovem.x (a0)+,fp0/fp1
        ]
    );
}

/// FMOVEM FPn 動的リスト（Dn マスク）のエンコード。
#[test]
fn test_fmovem_fpreg_dynamic_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.x\td0,(a0)\n\
\tfmovem.x\t(a0),d0\n\
\tfmovem.x\td0,-(a0)\n\
\tfmovem.x\t(a0)+,d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xF8, 0x00, // fmovem.x d0,(a0)
            0xF6, 0x10, 0xD8, 0x00, // fmovem.x (a0),d0
            0xF6, 0x20, 0xE8, 0x00, // fmovem.x d0,-(a0)
            0xF6, 0x18, 0xD8, 0x00, // fmovem.x (a0)+,d0
        ]
    );
}

/// FMOVEM FPCR 複合指定（fpcr/fpsr 形式）のエンコード。
#[test]
fn test_fmovem_fpctrl_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.l\tfpcr/fpsr,(a0)\n\
\tfmovem.l\t(a0),fpcr/fpsr\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xB8, 0x00, // fmovem.l fpcr/fpsr,(a0)
            0xF6, 0x10, 0x98, 0x00, // fmovem.l (a0),fpcr/fpsr
        ]
    );
}

/// FSINCOS のエンコード（FPn/EA ソース + FPc:FPs 宛先）。
#[test]
fn test_fsincos_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfsincos.x\tfp0,fp1:fp2\n\
\tfsincos.x\t(a0),fp1:fp2\n\
\tfsincos.l\td0,fp3:fp4\n\
\tfsincos.x\tfp0,fp5:fp6\n\
\tfsincos.x\tfp3,fp1:fp2\n\
\tfsincos.x\tfp0,fp0:fp1\n\
\tfsincos.x\tfp0,fp1:fp0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x00, 0x01, 0x31,
            0xF6, 0x10, 0x49, 0x31,
            0xF6, 0x00, 0x42, 0x33,
            0xF6, 0x00, 0x03, 0x35,
            0xF6, 0x00, 0x0D, 0x31,
            0xF6, 0x00, 0x00, 0xB0,
            0xF6, 0x00, 0x00, 0x31,
        ]
    );
}

/// FBcc / FDBcc の基本エンコード（.w/.l と CPID 反映）。
#[test]
fn test_fbcc_fdbcc_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfbne.w\ttarget_w\n\
\tnop\n\
target_w:\n\
\tnop\n\
\tfbne.l\ttarget_l\n\
\tnop\n\
target_l:\n\
\tnop\n\
\tfdbne\td0,target_d\n\
\tnop\n\
target_d:\n\
\tnop\n\
\tfbgt.w\ttarget_g\n\
\tnop\n\
target_g:\n\
\tnop\n\
\tfdbgt\td0,target_g2\n\
\tnop\n\
target_g2:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x8E, 0x00, 0x04, // fbne.w target_w
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_w: nop
            0xF6, 0xCE, 0x00, 0x00, 0x00, 0x06, // fbne.l target_l
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_l: nop
            0xF6, 0x48, 0x00, 0x0E, 0x00, 0x04, // fdbne d0,target_d
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_d: nop
            0xF6, 0x92, 0x00, 0x04, // fbgt.w target_g
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_g: nop
            0xF6, 0x48, 0x00, 0x12, 0x00, 0x04, // fdbgt d0,target_g2
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_g2: nop
        ]
    );
}

/// FMOVECR は .x 以外のサイズを受け付けない。
#[test]
fn test_fmovecr_rejects_non_extend_size() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.68040\n\tfmovecr.l\t#1,fp0\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    match rhas::pass::assemble(&mut ctx) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

/// .if/.endif 条件アセンブル
#[test]
fn test_conditional_asm() {
    let src = b"\
\t.if\t1\n\
\tmove.b\td0,d1\n\
\t.endif\n\
\t.if\t0\n\
\tadd.w\td1,d2\n\
\t.endif\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // if 1 は実行、if 0 はスキップ → move.b だけ
    assert_eq!(text.bytes, [0x12, 0x00], "only move.b should be emitted");
}

// ─── Phase 8: マクロ処理 ──────────────────────────────────────────────────────

/// 引数なしマクロ
#[test]
fn test_macro_no_args() {
    let src = b"\
push_d0\t.macro\n\
\tmove.l\td0,-(sp)\n\
\t.endm\n\
\tpush_d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // move.l d0,-(sp) = 0x2F00 (2 bytes)
    assert_eq!(text.bytes, [0x2F, 0x00], "macro expansion: move.l d0,-(sp)");
}

/// 引数ありマクロ
#[test]
fn test_macro_with_args() {
    let src = b"\
push_reg\t.macro\treg\n\
\tmove.l\t&reg,-(sp)\n\
\t.endm\n\
\tpush_reg\td1\n\
\tpush_reg\td2\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // move.l d1,-(sp) = 0x2F01 (2 bytes)
    // move.l d2,-(sp) = 0x2F02 (2 bytes)
    assert_eq!(text.bytes.len(), 4, "two macro expansions = 4 bytes");
    assert_eq!(&text.bytes[0..2], &[0x2F, 0x01], "push d1");
    assert_eq!(&text.bytes[2..4], &[0x2F, 0x02], "push d2");
}

/// .rept 繰り返し
#[test]
fn test_rept() {
    let src = b"\
\t.rept\t3\n\
\tnop\n\
\t.endm\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // nop x 3 = 6 bytes
    assert_eq!(text.bytes, [0x4E, 0x71, 0x4E, 0x71, 0x4E, 0x71], ".rept 3 nop");
}

/// .irp 引数反復
#[test]
fn test_irp() {
    let src = b"\
\t.irp\treg,d0,d1,d2\n\
\tmoveq\t#0,&reg\n\
\t.endm\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // moveq #0,d0 = 0x7000 (2 bytes)
    // moveq #0,d1 = 0x7200 (2 bytes)
    // moveq #0,d2 = 0x7400 (2 bytes)
    assert_eq!(text.bytes.len(), 6, ".irp 3 iterations");
    assert_eq!(&text.bytes[0..2], &[0x70, 0x00], "moveq #0,d0");
    assert_eq!(&text.bytes[2..4], &[0x72, 0x00], "moveq #0,d1");
    assert_eq!(&text.bytes[4..6], &[0x74, 0x00], "moveq #0,d2");
}

/// .irpc 文字列反復
#[test]
fn test_irpc() {
    // 各文字を ASCII コードとして .dc.b で出力
    let src = b"\
\t.irpc\tc,abc\n\
\t.dc.b\t'&c'\n\
\t.endm\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1)
        .expect("text section missing");
    // 'a' = 0x61, 'b' = 0x62, 'c' = 0x63
    assert_eq!(text.bytes, [0x61, 0x62, 0x63], ".irpc abc");
}

// ─── Phase 10: PRNリストファイル生成 ──────────────────────────────────────────

/// -p オプションでPRNファイルが生成される
#[test]
fn test_prn_list_file() {
    use std::io::Write;
    use tempfile::NamedTempFile;

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");

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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    match rhas::pass::assemble(&mut ctx) {
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
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result14 = rhas::pass::assemble(&mut ctx14).expect("assemble");
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
    let result15 = rhas::pass::assemble(&mut ctx15).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    assert_eq!(bb_next, eb_idx.map(|v| v + 1));
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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
    assert_eq!(var_tag, tagok_idx);
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
    let result = rhas::pass::assemble(&mut ctx).expect("assemble");
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

/// `.request` は `$E001` レコードとして出力される。
#[test]
fn test_request_emits_e001_record() {
    let result = assemble_src(b"\t.request\t\"libfoo.r\"\n\tnop\n");
    assert_eq!(result.obj.request_files, vec![b"libfoo.r".to_vec()]);

    let found = result.obj_bytes.windows(2).any(|w| w == [0xE0, 0x01]);
    assert!(found, "E001 record should exist when .request is used");
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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx_a).expect("assemble a");
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
    rhas::pass::assemble(&mut ctx_b).expect("assemble b");
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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

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
    rhas::pass::assemble(&mut ctx).expect("assemble");

    let prn = std::fs::read(std::path::Path::new(
        std::str::from_utf8(&prn_path).unwrap()
    )).expect("read prn");
    assert!(!prn.contains(&0x0C), "no formfeed should be emitted when prn_no_page_ff=true");
}

// ─── -c4 最適化 ──────────────────────────────────────────────────────────────

#[test]
fn test_c4_cmpi0_to_tst() {
    let src = b"\tcmpi.l\t#0,d3\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // TST.L D3 = 0x4A83
    assert_eq!(text.bytes, [0x4A, 0x83]);
}

#[test]
fn test_c4_movea_l_imm_to_w() {
    let src = b"\tmovea.l\t#1234,a2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // MOVEA.W #1234,A2 = 0x347C 0x04D2
    assert_eq!(text.bytes, [0x34, 0x7C, 0x04, 0xD2]);
}

#[test]
fn test_c4_asl_imm1_to_add() {
    let src = b"\tasl.w\t#1,d2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // ADD.W D2,D2 = 0xD442
    assert_eq!(text.bytes, [0xD4, 0x42]);
}

#[test]
fn test_c4_clr_l_does_not_optimize_on_68020_plus() {
    let src = b"\t.68040\n\tclr.l\td0\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // 68020+ では CLR.L Dn は MOVEQ #0,Dn に変換しない
    assert_eq!(text.bytes, [0x42, 0x80]);
}

#[test]
fn test_c4_cmpa_zero_to_tst_on_68020_plus() {
    let src = b"\t.68040\n\tcmpa.l\t#0,a2\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // CMPA.L #0,A2 -> TST.L A2
    assert_eq!(text.bytes, [0x4A, 0x8A]);
}

#[test]
fn test_c4_lea_disp_to_addq() {
    let src = b"\tlea\t(4,a4),a4\n";
    let result = assemble_src_c4(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    // LEA (4,A4),A4 -> ADDQ.W #4,A4
    assert_eq!(text.bytes, [0x58, 0x4C]);
}

// ---- ColdFire CPU 選択 ----

#[test]
fn test_coldfire_cpu5200_directive() {
    let src = b"\t.5200\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu_number, 5200);
    assert_ne!(ctx.cpu_type & rhas::options::cpu::C520, 0);
}

#[test]
fn test_coldfire_cpu5300_directive() {
    let src = b"\t.5300\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu_number, 5300);
    assert_ne!(ctx.cpu_type & rhas::options::cpu::C530, 0);
}

#[test]
fn test_coldfire_cpu5400_directive() {
    let src = b"\t.5400\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu_number, 5400);
    assert_ne!(ctx.cpu_type & rhas::options::cpu::C540, 0);
}

// ---- .cpu 式指定 ----

#[test]
fn test_cpu_directive_68020() {
    let src = b"\t.cpu\t68020\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu_number, 68020);
    assert_ne!(ctx.cpu_type & rhas::options::cpu::C020, 0);
}

#[test]
fn test_cpu_directive_5200() {
    let src = b"\t.cpu\t5200\n\tnop\n";
    let (_result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.cpu_number, 5200);
    assert_ne!(ctx.cpu_type & rhas::options::cpu::C520, 0);
}

// ---- FBcc/FDBcc 外部参照 ----

#[test]
fn test_fbcc_xref_generates_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfbeq\tEXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    // 外部参照シンボルが登録されていること
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    // HLK バイナリに 0x65 リロケーションレコードが存在すること
    assert!(result.obj_bytes.windows(2).any(|w| w[0] == 0x65),
            "0x65 reloc record should exist");
}

#[test]
fn test_fdbcc_xref_generates_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfdbeq\td0,EXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    assert!(result.obj_bytes.windows(2).any(|w| w[0] == 0x65),
            "0x65 reloc record should exist");
}

// ---- Bcc.L / FBcc.L 外部参照 (RPN リロケーション) ----

#[test]
fn test_bcc_long_xref_generates_rpn_reloc() {
    let src = b"\
\t.68020\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tbeq.l\tEXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    // RPN リロケーション: $80FF (xref push) + $A00F (subtract) + $92xx (long store)
    let bytes = &result.obj_bytes;
    assert!(bytes.windows(2).any(|w| w[0] == 0x80 && w[1] == 0xFF),
            "0x80FF xref RPN entry should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0xA0 && w[1] == 0x0F),
            "0xA00F subtract operator should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0x92 && w[1] == 0x00),
            "0x9200 long size terminator should exist");
}

#[test]
fn test_fbcc_long_xref_generates_rpn_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfbeq.l\tEXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    let bytes = &result.obj_bytes;
    assert!(bytes.windows(2).any(|w| w[0] == 0x80 && w[1] == 0xFF),
            "0x80FF xref RPN entry should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0xA0 && w[1] == 0x0F),
            "0xA00F subtract operator should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0x92 && w[1] == 0x00),
            "0x9200 long size terminator should exist");
}
