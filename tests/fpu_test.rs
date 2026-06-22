mod common;
use common::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_fpid_sets_id_and_can_disable_fpu() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.fpid\t3\n\t.fpid\t-1\n\tnop\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let _ = rhas::pass::assemble(&mut ctx).expect("assemble");
    assert_eq!(ctx.fpid, 3);
    assert_eq!(ctx.cpu.features & rhas::options::cpu::CFPP, 0, "negative .fpid should disable CFPP");
}

/// `.fpid` は 0..7 以外を拒否する。
#[test]
fn test_fpid_rejects_out_of_range() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.fpid\t8\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    match rhas::pass::assemble(&mut ctx) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

/// FPU コア命令のエンコード（FNOP/FMOVE/FADD/FCMP/FTST/FMOVECR/FSAVE/FRESTORE）。
#[test]
fn test_fpu_core_instruction_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfnop\n\
\tfmove.x\tfp0,fp1\n\
\tfadd.l\td0,fp1\n\
\tfcmp.x\tfp2,fp1\n\
\tftst\t(a0)\n\
\tfmovecr\t#1,fp2\n\
\tfsave\t(a0)\n\
\tfrestore\t(a0)\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF6, 0x80, 0x00, 0x00, // fnop
            0xF6, 0x00, 0x00, 0x80, // fmove.x fp0,fp1
            0xF6, 0x00, 0x40, 0xA2, // fadd.l d0,fp1
            0xF6, 0x00, 0x08, 0xB8, // fcmp.x fp2,fp1
            0xF6, 0x10, 0x48, 0x3A, // ftst (a0) (default .x)
            0xF6, 0x00, 0x5D, 0x01, // fmovecr #1,fp2
            0xF7, 0x10,             // fsave (a0)
            0xF7, 0x50,             // frestore (a0)
        ]
    );
}

/// FMOVE のデフォルトサイズはメモリ経由で .x になる。
#[test]
fn test_fmove_default_size_is_extend_for_memory_forms() {
    let src = b"\
\t.68040\n\
\tfmove\t(a0),fp1\n\
\tfmove\tfp1,(a0)\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF2, 0x10, 0x48, 0x80,
            0xF2, 0x10, 0x68, 0x80,
        ]
    );
}

/// FMOVEM (control registers) のエンコード。
#[test]
fn test_fmovem_control_register_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem\tfpcr,(a0)\n\
\tfmovem\tfpsr,(a0)\n\
\tfmovem\tfpiar,(a0)\n\
\tfmovem\t(a0),fpcr\n\
\tfmovem\t(a0),fpsr\n\
\tfmovem\t(a0),fpiar\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text section");
    assert_eq!(
        text.bytes,
        vec![
            0xF6, 0x10, 0xB0, 0x00,
            0xF6, 0x10, 0xA8, 0x00,
            0xF6, 0x10, 0xA4, 0x00,
            0xF6, 0x10, 0x90, 0x00,
            0xF6, 0x10, 0x88, 0x00,
            0xF6, 0x10, 0x84, 0x00,
        ]
    );
}

/// FMOVEM FPn レジスタリスト（静的リスト）のエンコード。
#[test]
fn test_fmovem_fpreg_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.x\tfp0/fp1,(a0)\n\
\tfmovem.x\t(a0),fp0/fp1\n\
\tfmovem.x\tfp0/fp1,-(a0)\n\
\tfmovem.x\t(a0)+,fp0/fp1\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xF0, 0xC0, // fmovem.x fp0/fp1,(a0)
            0xF6, 0x10, 0xD0, 0xC0, // fmovem.x (a0),fp0/fp1
            0xF6, 0x20, 0xE0, 0x03, // fmovem.x fp0/fp1,-(a0)
            0xF6, 0x18, 0xD0, 0xC0, // fmovem.x (a0)+,fp0/fp1
        ]
    );
}

/// FMOVEM FPn 動的リスト（Dn マスク）のエンコード。
#[test]
fn test_fmovem_fpreg_dynamic_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.x\td0,(a0)\n\
\tfmovem.x\t(a0),d0\n\
\tfmovem.x\td0,-(a0)\n\
\tfmovem.x\t(a0)+,d0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xF8, 0x00, // fmovem.x d0,(a0)
            0xF6, 0x10, 0xD8, 0x00, // fmovem.x (a0),d0
            0xF6, 0x20, 0xE8, 0x00, // fmovem.x d0,-(a0)
            0xF6, 0x18, 0xD8, 0x00, // fmovem.x (a0)+,d0
        ]
    );
}

/// FMOVEM FPCR 複合指定（fpcr/fpsr 形式）のエンコード。
#[test]
fn test_fmovem_fpctrl_list_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfmovem.l\tfpcr/fpsr,(a0)\n\
\tfmovem.l\t(a0),fpcr/fpsr\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x10, 0xB8, 0x00, // fmovem.l fpcr/fpsr,(a0)
            0xF6, 0x10, 0x98, 0x00, // fmovem.l (a0),fpcr/fpsr
        ]
    );
}

/// FSINCOS のエンコード（FPn/EA ソース + FPc:FPs 宛先）。
#[test]
fn test_fsincos_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfsincos.x\tfp0,fp1:fp2\n\
\tfsincos.x\t(a0),fp1:fp2\n\
\tfsincos.l\td0,fp3:fp4\n\
\tfsincos.x\tfp0,fp5:fp6\n\
\tfsincos.x\tfp3,fp1:fp2\n\
\tfsincos.x\tfp0,fp0:fp1\n\
\tfsincos.x\tfp0,fp1:fp0\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x00, 0x01, 0x31,
            0xF6, 0x10, 0x49, 0x31,
            0xF6, 0x00, 0x42, 0x33,
            0xF6, 0x00, 0x03, 0x35,
            0xF6, 0x00, 0x0D, 0x31,
            0xF6, 0x00, 0x00, 0xB0,
            0xF6, 0x00, 0x00, 0x31,
        ]
    );
}

/// FBcc / FDBcc の基本エンコード（.w/.l と CPID 反映）。
#[test]
fn test_fbcc_fdbcc_encoding() {
    let src = b"\
\t.68040\n\
\t.fpid\t3\n\
\tfbne.w\ttarget_w\n\
\tnop\n\
target_w:\n\
\tnop\n\
\tfbne.l\ttarget_l\n\
\tnop\n\
target_l:\n\
\tnop\n\
\tfdbne\td0,target_d\n\
\tnop\n\
target_d:\n\
\tnop\n\
\tfbgt.w\ttarget_g\n\
\tnop\n\
target_g:\n\
\tnop\n\
\tfdbgt\td0,target_g2\n\
\tnop\n\
target_g2:\n\
\tnop\n\
";
    let result = assemble_src(src);
    let text = result.obj.sections.iter().find(|s| s.id == 1).expect("text");
    assert_eq!(
        text.bytes,
        [
            0xF6, 0x8E, 0x00, 0x04, // fbne.w target_w
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_w: nop
            0xF6, 0xCE, 0x00, 0x00, 0x00, 0x06, // fbne.l target_l
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_l: nop
            0xF6, 0x48, 0x00, 0x0E, 0x00, 0x04, // fdbne d0,target_d
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_d: nop
            0xF6, 0x92, 0x00, 0x04, // fbgt.w target_g
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_g: nop
            0xF6, 0x48, 0x00, 0x12, 0x00, 0x04, // fdbgt d0,target_g2
            0x4E, 0x71,             // nop
            0x4E, 0x71,             // target_g2: nop
        ]
    );
}

/// FMOVECR は .x 以外のサイズを受け付けない。
#[test]
fn test_fmovecr_rejects_non_extend_size() {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(b"\t.68040\n\tfmovecr.l\t#1,fp0\n").expect("write");
    let path = f.path().to_str().expect("path").as_bytes().to_vec();
    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    match rhas::pass::assemble(&mut ctx) {
        Err(rhas::pass::AssembleError::HasErrors(n)) => assert!(n >= 1),
        Err(other) => panic!("unexpected error: {:?}", other),
        Ok(_) => panic!("assemble should fail"),
    }
}

#[test]
fn test_fbcc_xref_generates_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfbeq\tEXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    assert!(result.obj_bytes.windows(2).any(|w| w[0] == 0x65),
            "0x65 reloc record should exist");
}

#[test]
fn test_fdbcc_xref_generates_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfdbeq\td0,EXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    assert!(result.obj_bytes.windows(2).any(|w| w[0] == 0x65),
            "0x65 reloc record should exist");
}

#[test]
fn test_fbcc_long_xref_generates_rpn_reloc() {
    let src = b"\
\t.68040\n\
\t.xref\tEXTLABEL\n\
\t.text\n\
\tfbeq.l\tEXTLABEL\n";
    let (result, ctx) = assemble_with_ctx(src);
    assert_eq!(ctx.num_errors, 0, "should assemble without errors");
    assert!(result.obj.ext_syms.iter().any(|s| s.name.as_slice() == b"EXTLABEL"),
            "EXTLABEL should be in ext_syms");
    let bytes = &result.obj_bytes;
    assert!(bytes.windows(2).any(|w| w[0] == 0x80 && w[1] == 0xFF),
            "0x80FF xref RPN entry should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0xA0 && w[1] == 0x0F),
            "0xA00F subtract operator should exist");
    assert!(bytes.windows(2).any(|w| w[0] == 0x92 && w[1] == 0x00),
            "0x9200 long size terminator should exist");
}
