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

#[test]
fn test_error_message_cpu_invalid_number() {
    let out = run_rhas(b"\t.cpu\t99999\n");
    assert!(!out.status.success(), "assemble should fail");

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(stderr.contains("未対応の cpu です"), "stderr: {}", stderr);
    assert!(stdout.contains("エラーが 1 個ありました"));
}

// ─── CPU gating: 68020+ on 68000 ─────────────────────────────────────────────

#[test]
fn test_error_extb_on_68000() {
    // EXTB.L is 68020+ only; on 68000 it should error
    let out = run_rhas(b"\textb.l\td0\n");
    assert!(!out.status.success(), "EXTB.L should fail on 68000");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("命令が解釈できません") || stderr.contains("記述が間違っています"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not gate Bcc.L to 68020+; assembles without error
fn test_error_bcc_long_on_68000() {
    // Bcc.L is 68020+ only; on 68000 it should error
    let out = run_rhas(
        b"\tbra.l\ttarget\n\
        \t.ds.b\t256\n\
        target:\n\tnop\n",
    );
    assert!(!out.status.success(), "Bcc.L should fail on 68000");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ロングワードサイズの相対分岐はできません"),
        "stderr: {}", stderr
    );
}

#[test]
fn test_error_pack_on_68000() {
    // PACK is 68020+ only
    let out = run_rhas(b"\tpack\td0,d1,#0\n");
    assert!(!out.status.success(), "PACK should fail on 68000");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("命令が解釈できません") || stderr.contains("記述が間違っています"),
        "stderr: {}", stderr
    );
}

#[test]
fn test_ok_extb_on_68020() {
    // EXTB.L should succeed on 68020
    let out = run_rhas(b"\t.cpu\t68020\n\textb.l\td0\n");
    assert!(out.status.success(), "EXTB.L should succeed on 68020");
}

#[test]
fn test_ok_bcc_long_on_68020() {
    // Bcc.L should succeed on 68020
    let out = run_rhas(
        b"\t.cpu\t68020\n\
        \tbra.l\ttarget\n\
        \t.ds.b\t256\n\
        target:\n\tnop\n",
    );
    assert!(out.status.success(), "Bcc.L should succeed on 68020");
}

#[test]
#[ignore] // BUG: rhas does not gate CHK.L to 68020+; assembles without error
fn test_error_chk_long_on_68000() {
    // CHK.L is 68020+; on 68000 only CHK.W is valid
    let out = run_rhas(b"\tchk.l\td0,d1\n");
    assert!(!out.status.success(), "CHK.L should fail on 68000");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ロングワードサイズは指定できません"),
        "stderr: {}", stderr
    );
}

// ─── Range / overflow errors ──────────────────────────────────────────────────

#[test]
#[ignore] // BUG: rhas does not check MOVEQ range; assembles without error
fn test_error_moveq_overflow() {
    // MOVEQ immediate must be -128..127
    let out = run_rhas(b"\tmoveq\t#128,d0\n");
    assert!(!out.status.success(), "MOVEQ #128 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("-128～127 の範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of MOVEQ range
fn test_error_moveq_negative_overflow() {
    let out = run_rhas(b"\tmoveq\t#-129,d0\n");
    assert!(!out.status.success(), "MOVEQ #-129 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("-128～127 の範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of ADDQ range
fn test_error_addq_overflow() {
    // ADDQ immediate must be 1..8
    let out = run_rhas(b"\taddq.w\t#9,d0\n");
    assert!(!out.status.success(), "ADDQ #9 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1～8 の範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of ADDQ range
fn test_error_addq_zero() {
    let out = run_rhas(b"\taddq.w\t#0,d0\n");
    assert!(!out.status.success(), "ADDQ #0 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("1～8 の範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not detect bra.s out of range; assembles without error
fn test_error_bra_short_out_of_range() {
    // bra.s can only reach -128..+127 from next instruction
    let mut src = Vec::new();
    src.extend_from_slice(b"\tbra.s\ttarget\n");
    // Pad with 128 NOP (2 bytes each = 256 bytes → out of .s range)
    for _ in 0..128 {
        src.extend_from_slice(b"\tnop\n");
    }
    src.extend_from_slice(b"target:\n\tnop\n");
    let out = run_rhas(&src);
    assert!(!out.status.success(), "bra.s out of range should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("オフセットが範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of shift count range
fn test_error_shift_count_overflow() {
    // Immediate shift count must be 1..8
    let out = run_rhas(b"\tasl.w\t#9,d0\n");
    assert!(!out.status.success(), "ASL #9 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("カウントが") && stderr.contains("範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of shift count range
fn test_error_shift_count_zero() {
    let out = run_rhas(b"\tasl.w\t#0,d0\n");
    assert!(!out.status.success(), "ASL #0 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("カウントが") && stderr.contains("範囲外です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not detect division by zero; assembles without error
fn test_error_div_zero() {
    let out = run_rhas(b"x\t.equ\t10/0\n");
    assert!(!out.status.success(), "division by zero should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("0 で除算しました"),
        "stderr: {}", stderr
    );
}

// ─── Size restriction errors ──────────────────────────────────────────────────

#[test]
#[ignore] // BUG: rhas does not reject An byte access; assembles without error
fn test_error_address_register_byte() {
    // Address registers cannot be accessed in byte size
    let out = run_rhas(b"\tmove.b\td0,a0\n");
    assert!(!out.status.success(), "move.b to An should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("バイトサイズでアクセスできません"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas errors but with generic message instead of memory shift size
fn test_error_memory_shift_non_word() {
    // Memory shift/rotate is word-only
    let out = run_rhas(b"\tasl.l\t(a0)\n");
    assert!(!out.status.success(), "ASL.L (a0) should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ワードサイズのみ指定可能です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject memory bit non-byte; assembles without error
fn test_error_memory_bit_non_byte() {
    // Memory bit operations are byte-only
    let out = run_rhas(b"\tbclr.w\t#3,(a0)\n");
    assert!(!out.status.success(), "BCLR.W (a0) should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("バイトサイズのみ指定可能です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject register bit non-long; assembles without error
fn test_error_register_bit_non_long() {
    // Register bit operations are long-only
    let out = run_rhas(b"\tbtst.w\t#3,d0\n");
    assert!(!out.status.success(), "BTST.W d0 should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ロングワードサイズのみ指定可能です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject MOVE.L to CCR; assembles without error
fn test_error_move_to_ccr_non_word() {
    // MOVE to CCR must be word size
    let out = run_rhas(b"\tmove.l\td0,ccr\n");
    assert!(!out.status.success(), "MOVE.L to CCR should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ワードサイズのみ指定可能です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject MOVE.L to SR; assembles without error
fn test_error_move_to_sr_non_word() {
    // MOVE to SR must be word size
    let out = run_rhas(b"\tmove.l\td0,sr\n");
    assert!(!out.status.success(), "MOVE.L to SR should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ワードサイズのみ指定可能です"),
        "stderr: {}", stderr
    );
}

// ─── Symbol / expression errors ───────────────────────────────────────────────

#[test]
#[ignore] // BUG: rhas does not reject undefined symbol; assembles without error
fn test_error_undefined_symbol() {
    let out = run_rhas(b"\tmove.l\tnoexist,d0\n");
    assert!(!out.status.success(), "undefined symbol should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("未定義です"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject duplicate labels; assembles without error
fn test_error_symbol_redefinition() {
    let out = run_rhas(b"dup:\tnop\ndup:\tnop\n");
    assert!(!out.status.success(), "duplicate label should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("既に定義されています"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject unclosed string; assembles without error
fn test_error_unclosed_string() {
    let out = run_rhas(b"\t.dc.b\t\"hello\n");
    assert!(!out.status.success(), "unclosed string should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("閉じていません"),
        "stderr: {}", stderr
    );
}

// ─── Macro errors ─────────────────────────────────────────────────────────────

#[test]
#[ignore] // BUG: rhas does not reject orphan .endm; assembles without error
fn test_error_endm_without_macro() {
    let out = run_rhas(b"\t.endm\n");
    assert!(!out.status.success(), ".endm without .macro should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".endm に対応する .macro がありません"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject orphan .else; assembles without error
fn test_error_else_without_if() {
    let out = run_rhas(b"\t.else\n");
    assert!(!out.status.success(), ".else without .if should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(".else に対応する .if がありません"),
        "stderr: {}", stderr
    );
}

#[test]
#[ignore] // BUG: rhas does not reject orphan .endif; assembles without error
fn test_error_endif_without_if() {
    let out = run_rhas(b"\t.endif\n");
    assert!(!out.status.success(), ".endif without .if should fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("対応する .if がありません"),
        "stderr: {}", stderr
    );
}
