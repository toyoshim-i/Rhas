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

fn run_rhas_with_args(src: &[u8], args: &[&str]) -> std::process::Output {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rhas"));
    cmd.args(args);
    cmd.arg(f.path());
    cmd.output().expect("run rhas")
}

#[test]
fn test_error_message_invalid_size_fmovecr() {
    let out = run_rhas(b"\t.68040\n\tfmovecr.l\t#1,fp0\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("記述が間違っています"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_invalid_operand_file_directive() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.endef\t1\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("不正なオペランドです"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_invalid_expr_val_directive() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.val\t)\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("記述が間違っています"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_scd_boundary_scl_range() {
    let out = run_rhas(b"\t.file\t\"main.c\"\n\t.scl\t256\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("不正な値です"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_fpid_boundary() {
    let out = run_rhas(b"\t.fpid\t8\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("不正な値です"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

#[test]
fn test_error_message_fmovem_size_boundaries() {
    let out = run_rhas(
        b"\t.68040\n\
\t.fpid\t3\n\
\tfmovem.l\tfp0/fp1,(a0)\n\
\tfmovem.x\tfpcr,(a0)\n\
\tfmovem.l\td0,(a0)\n\
\tfmovem.b\tfp0/fp1,(a0)\n\
\tfmovem.b\tfpcr,(a0)\n",
    );
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert_eq!(stderr.matches("記述が間違っています").count(), 5, "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 5 個ありました"));
}

#[test]
fn test_warning_level_zero_suppresses_offsym_warning() {
    let src = b"\
start:\n\
\tnop\n\
\t.offsym\t0,start\n";
    let out = run_rhas_with_args(src, &["-w0"]);
    assert!(out.status.success(), "assemble should succeed");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains(".offsym により既存シンボルを上書きしました"));
}

#[test]
fn test_warning_message_offsym_uses_warn_table_with_symbol() {
    let src = b"\
start:\n\
\tnop\n\
\t.offsym\t0,start\n";
    let out = run_rhas_with_args(src, &["-w1"]);
    assert!(out.status.success(), "assemble should succeed");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("シンボル start を .offsym で上書きしました"));
}
