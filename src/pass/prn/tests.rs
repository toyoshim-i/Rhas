use super::*;

const DEFAULT_PRN_CODE_WIDTH: usize = 16; // コード部文字数 (8バイト = 16 hex chars)
const DEFAULT_PRN_LINE_WIDTH: usize = 136; // 全体幅

#[test]
fn test_format_simple_line() {
    let line = PrnLine {
        line_num: 1,
        location: 0,
        section: 1,
        bytes: vec![0x12, 0x00],
        text: b"        move.b  d0,d1".to_vec(),
        is_macro: false,
    };
    let out = format_prn(
        &[line],
        b"",
        b"",
        DEFAULT_PRN_LINE_WIDTH,
        DEFAULT_PRN_CODE_WIDTH,
        false,
        58,
    );
    let s = crate::utils::bytes_to_string(&out);
    // Check structure: "    1 00000000  1200            ..."
    assert!(s.contains("    1 00000000 "));
    assert!(s.contains("1200"));
    assert!(s.contains("move.b"));
}

#[test]
fn test_format_long_code() {
    // 10 bytes = 20 hex chars > code_width(16)
    let line = PrnLine {
        line_num: 2,
        location: 0x100,
        section: 1,
        bytes: vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99],
        text: b"        long instruction".to_vec(),
        is_macro: false,
    };
    let out = format_prn(
        &[line],
        b"",
        b"",
        DEFAULT_PRN_LINE_WIDTH,
        DEFAULT_PRN_CODE_WIDTH,
        false,
        58,
    );
    let s = crate::utils::bytes_to_string(&out);
    // Should have 2 lines (first 8 bytes + last 2 bytes)
    assert_eq!(s.lines().count(), 2);
}

#[test]
fn test_format_macro_line() {
    let line = PrnLine {
        line_num: 5,
        location: 0x10,
        section: 1,
        bytes: vec![0x4E, 0x71],
        text: b"        macro_call".to_vec(),
        is_macro: true,
    };
    let out = format_prn(
        &[line],
        b"",
        b"",
        DEFAULT_PRN_LINE_WIDTH,
        DEFAULT_PRN_CODE_WIDTH,
        false,
        58,
    );
    let s = crate::utils::bytes_to_string(&out);
    // '*' marker for macro
    assert!(s.contains('*'));
}

#[test]
fn test_format_title_header() {
    let out = format_prn(
        &[],
        b"MyTitle",
        b"MySub",
        DEFAULT_PRN_LINE_WIDTH,
        DEFAULT_PRN_CODE_WIDTH,
        false,
        58,
    );
    let s = crate::utils::bytes_to_string(&out);
    assert!(s.contains("MyTitle"));
    assert!(s.contains("MySub"));
}

#[test]
fn test_page_break_directive_detection() {
    assert!(is_page_break_directive(b"\t.page"));
    assert!(is_page_break_directive(b"   .PAGE +"));
    assert!(!is_page_break_directive(b"\t.page\t60"));
    assert!(!is_page_break_directive(b"\t.pagex"));
    assert!(!is_page_break_directive(b";.page"));
}

#[test]
fn test_auto_page_break_by_line_limit() {
    let mut lines = Vec::new();
    for i in 0..12u32 {
        lines.push(PrnLine {
            line_num: i + 1,
            location: i * 2,
            section: 1,
            bytes: vec![0x4E, 0x71],
            text: b"nop".to_vec(),
            is_macro: false,
        });
    }
    let out = format_prn(
        &lines,
        b"",
        b"",
        DEFAULT_PRN_LINE_WIDTH,
        DEFAULT_PRN_CODE_WIDTH,
        false,
        10,
    );
    assert!(out.contains(&0x0C));
}
