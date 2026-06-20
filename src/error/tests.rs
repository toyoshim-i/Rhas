use super::*;

#[test]
fn test_error_message() {
    assert_eq!(ErrorCode::BadOpe.message(), "命令が解釈できません");
    assert_eq!(ErrorCode::UndefSym.message(), "シンボル %s が未定義です");
}

#[test]
fn test_format_message() {
    let mut buf = Vec::new();
    let pos = SourcePos::new(b"test.s".to_vec(), 1);
    // format_message is private helper in printer.rs, but we can verify it indirectly via print_error
    print_error(&mut buf, &pos, ErrorCode::UndefSym, Some(b"LABEL"));
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("シンボル LABEL が未定義です"));
}

#[test]
fn test_format_message_no_sym() {
    let mut buf = Vec::new();
    let pos = SourcePos::new(b"test.s".to_vec(), 1);
    print_error(&mut buf, &pos, ErrorCode::Expr, None);
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("記述が間違っています"));
}

#[test]
fn test_warn_level() {
    assert_eq!(warn_default_level(warn::ABS), 4);
    assert_eq!(warn_default_level(warn::REGL), 1);
}

#[test]
fn test_error_context_new() {
    let pos = SourcePos::new(b"test.s".to_vec(), 10);
    let ctx = ErrorContext::new(pos.clone(), ErrorCode::BadOpe, None);
    assert_eq!(ctx.code, ErrorCode::BadOpe);
    assert_eq!(ctx.pos.line, 10);
    assert!(ctx.symbol.is_none());
}

#[test]
fn test_error_context_with_symbol() {
    let pos = SourcePos::new(b"test.s".to_vec(), 10);
    let ctx = ErrorContext::with_symbol(pos, ErrorCode::UndefSym, b"LABEL");
    assert_eq!(ctx.code, ErrorCode::UndefSym);
    assert_eq!(ctx.symbol, Some(b"LABEL".to_vec()));
}

#[test]
fn test_warn_context_new() {
    let pos = SourcePos::new(b"test.s".to_vec(), 20);
    let ctx = WarnContext::new(pos.clone(), warn::ABS, None);
    assert_eq!(ctx.code, warn::ABS);
    assert_eq!(ctx.pos.line, 20);
    assert!(ctx.symbol.is_none());
}

#[test]
fn test_warn_context_with_symbol() {
    let pos = SourcePos::new(b"test.s".to_vec(), 20);
    let ctx = WarnContext::with_symbol(pos, warn::REDEF_SET, b"VAR");
    assert_eq!(ctx.code, warn::REDEF_SET);
    assert_eq!(ctx.symbol, Some(b"VAR".to_vec()));
}

#[test]
fn test_print_error_context() {
    let pos = SourcePos::new(b"prog.s".to_vec(), 42);
    let ctx = ErrorContext::with_symbol(pos, ErrorCode::UndefSym, b"FOO");
    let mut buf = Vec::new();
    print_error_context(&mut buf, &ctx);
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("prog.s"));
    assert!(output.contains("42"));
    assert!(output.contains("Error"));
    assert!(output.contains("FOO"));
}

#[test]
fn test_print_warning_context() {
    let pos = SourcePos::new(b"prog.s".to_vec(), 50);
    let ctx = WarnContext::new(pos, warn::ABS, None);
    let mut buf = Vec::new();
    print_warning_context(&mut buf, &ctx, 5);
    let output = String::from_utf8(buf).unwrap();
    assert!(output.contains("prog.s"));
    assert!(output.contains("50"));
    assert!(output.contains("Warning"));
}
