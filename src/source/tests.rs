use super::*;
use std::path::PathBuf;

fn make_buf(data: &[u8]) -> SourceBuf {
    SourceBuf::from_bytes(data.to_vec(), PathBuf::from("test.s"))
}

#[test]
fn test_read_line_lf() {
    let mut buf = make_buf(b"line1\nline2\n");
    assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
    assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
    assert_eq!(buf.read_line(), None);
}

#[test]
fn test_read_line_crlf() {
    let mut buf = make_buf(b"line1\r\nline2\r\n");
    assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
    assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
    assert_eq!(buf.read_line(), None);
}

#[test]
fn test_read_line_cr_only() {
    let mut buf = make_buf(b"line1\rline2\r");
    assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
    assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
    assert_eq!(buf.read_line(), None);
}

#[test]
fn test_read_line_no_trailing_newline() {
    let mut buf = make_buf(b"only");
    assert_eq!(buf.read_line(), Some(b"only".to_vec()));
    assert_eq!(buf.read_line(), None);
}

#[test]
fn test_line_number_tracking() {
    let mut buf = make_buf(b"a\nb\nc\n");
    buf.read_line();
    assert_eq!(buf.line, 1);
    buf.read_line();
    assert_eq!(buf.line, 2);
    buf.read_line();
    assert_eq!(buf.line, 3);
}

#[test]
fn test_include_paths_parse() {
    let raw = b"path/a\0path/b\0".to_vec();
    let paths = parse_include_paths(Some(&raw));
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], PathBuf::from("path/a"));
    assert_eq!(paths[1], PathBuf::from("path/b"));
}
