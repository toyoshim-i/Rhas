/// End-to-end integration tests
///
/// アセンブラの 3 パスパイプライン全体を検証する。

use std::io::Write;
use tempfile::NamedTempFile;

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
