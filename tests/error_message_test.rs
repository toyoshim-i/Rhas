use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

fn run_rhas(src: &[u8]) -> std::process::Output {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");

    Command::new(env!("CARGO_BIN_EXE_rhas"))
        .arg(f.path())
        .output()
        .expect("run rhas")
}

#[test]
fn test_error_message_invalid_size_fmovecr() {
    let out = run_rhas(b"\t.68040\n\tfmovecr.l\t#1,fp0\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("命令のエンコードに失敗しました: InvalidSize"));
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_invalid_operand_file_directive() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.endef\t1\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains(".endef にオペランドは指定できません"));
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_invalid_expr_val_directive() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.val\t)\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains(".val の式が不正です"));
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_scd_boundary_scl_range() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.scl\t256\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains(".scl の値は -1 または 0..255 で指定してください"));
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_fpid_boundary() {
    let out = run_rhas(b"\t.fpid\t8\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains(".fpid の値は 0..7 で指定してください"));
    assert!(stdout.contains("エラーが 1 個ありました"));
}
