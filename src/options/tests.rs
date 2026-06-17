use super::*;

#[test]
fn test_defaults() {
    let opts = Options::default();
    assert_eq!(opts.cpu.number, 68000);
    assert_eq!(opts.cpu.features, cpu::C000);
    assert_eq!(opts.local_len_max, 4);
    assert_eq!(opts.local_num_max, 10000);
}

#[test]
fn test_basic_parse() {
    let result = parse_args(["source.s"], false);
    let opts = result.unwrap();
    assert_eq!(opts.source_file, Some(b"source.s".to_vec()));
    assert!(!opts.all_xref);
}

#[test]
fn test_parse_cu() {
    // -c4 -u
    let result = parse_args(["-c4", "-u", "source.s"], false);
    let opts = result.unwrap();
    assert!(opts.opt_clr);
    assert!(opts.all_xref);
}

#[test]
fn test_no_source() {
    let result = parse_args::<[&str; 0], &str>([], false);
    assert!(matches!(result, Err(ParseError::Usage(_))));
}

#[test]
fn test_c_option() {
    let result = parse_args(["-c4", "foo.s"], false);
    let opts = result.unwrap();
    assert!(opts.opt_clr);
    assert!(!opts.compat_mode);
    assert!(!opts.no_abs_short);
}

#[test]
fn test_m_option() {
    let result = parse_args(["-m68020", "foo.s"], false);
    let opts = result.unwrap();
    assert_eq!(opts.cpu.number, 68020);
    assert_eq!(opts.cpu.features, cpu::C020);
}

#[test]
fn test_w_option() {
    let result = parse_args(["-w0", "foo.s"], false);
    let opts = result.unwrap();
    assert_eq!(opts.effective_warn_level(), 0);
}
