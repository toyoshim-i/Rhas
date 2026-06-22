mod common;
use common::*;

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
