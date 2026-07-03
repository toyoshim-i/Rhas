mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

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
fn test_pass3_displacement_overflow_error() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\
\t.text\n\
\tbra.s\tlabel\n\
\t.ds.b\t200\n\
label:\n\
\tnop\n").expect("write");
    let path = f.path().to_path_buf();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    match rhas::pass::assemble(&mut ctx, &mut reporter) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => {
            assert_eq!(n, 1);
            assert_eq!(reporter.errors.len(), 1);
            assert_eq!(reporter.errors[0].code, rhas::error::ErrorCode::IlRelOutside);
        }
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail due to displacement overflow in pass 3"),
    }
}
