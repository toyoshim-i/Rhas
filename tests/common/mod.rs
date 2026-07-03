#![allow(dead_code)]

use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// ソーステキストからオブジェクトコードを生成して HLK バイト列を返す。
pub fn assemble_src(src: &[u8]) -> rhas::pass::AssembleResult {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_path_buf();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble")
}

/// -c4 相当の拡張最適化を有効にしてアセンブルする。
pub fn assemble_src_c4(src: &[u8]) -> rhas::pass::AssembleResult {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_path_buf();

    let opts = rhas::options::Options {
        source_file: Some(path),
        opt_clr: true,
        opt_movea: true,
        opt_adda_suba: true,
        opt_cmpa: true,
        opt_lea: true,
        opt_asl: true,
        opt_cmp0: true,
        opt_move0: true,
        opt_cmpi0: true,
        opt_sub_addi0: true,
        opt_bsr: true,
        opt_jmp_jsr: true,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble")
}

/// コンテキストを返しつつアセンブルする（pass遷移確認用）。
pub fn assemble_with_ctx(src: &[u8]) -> (rhas::pass::AssembleResult, rhas::context::AssemblyContext) {
    let mut f = NamedTempFile::new().expect("tempfile");
    f.write_all(src).expect("write");
    let path = f.path().to_path_buf();

    let opts = rhas::options::Options {
        source_file: Some(path),
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    let result = rhas::pass::assemble(&mut ctx, &mut reporter).expect("assemble");
    (result, ctx)
}

/// ソーステキストを Pass1 のみ実行し、生成された TempRecord を返す。
pub fn pass1_records(src: &[u8], make_sym_deb: bool) -> Vec<rhas::pass::temp::TempRecord> {
    let buf = rhas::source::SourceBuf::from_bytes(src.to_vec(), PathBuf::from("inline.s"));
    let mut source = rhas::source::SourceStack::new(buf, vec![]);
    let opts = rhas::options::Options {
        make_sym_deb,
        ..Default::default()
    };
    let mut ctx = rhas::context::AssemblyContext::new(opts);
    let mut sym = rhas::symbol::SymbolTable::new(false);
    let mut reporter = rhas::error::BufferReporter::new(ctx.effective_warn_level());
    rhas::pass::pass1::pass1(&mut source, &mut ctx, &mut sym, &mut reporter)
}

pub fn find_scd_footer(bytes: &[u8]) -> (usize, usize, usize, usize) {
    let end_pos = (0..bytes.len().saturating_sub(14))
        .find(|&i| {
            if bytes[i] != 0x00 || bytes[i + 1] != 0x00 {
                return false;
            }
            let p = i + 2;
            let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
            let scd_len = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize;
            let exname_len = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]) as usize;
            p + 12 + line_len + scd_len + exname_len == bytes.len()
        })
        .expect("0000 terminator");
    let p = end_pos + 2;
    let line_len = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]) as usize;
    let scd_len = u32::from_be_bytes([bytes[p + 4], bytes[p + 5], bytes[p + 6], bytes[p + 7]]) as usize;
    let exname_len = u32::from_be_bytes([bytes[p + 8], bytes[p + 9], bytes[p + 10], bytes[p + 11]]) as usize;
    (p, line_len, scd_len, exname_len)
}

pub fn scd_entry_offsets(bytes: &[u8], p: usize, line_len: usize, scd_len: usize) -> Vec<usize> {
    let scd_base = p + 12 + line_len;
    let mut out = Vec::new();
    let mut cur = scd_base;
    let end = scd_base + scd_len;
    while cur < end {
        out.push(cur);
        let len = bytes[cur + 17] as usize;
        cur += (len + 1) * 18;
    }
    assert_eq!(cur, end, "SCD entry stream length mismatch");
    out
}

/// Compute the record (18-byte slot) index for entry at `entry_idx`.
pub fn scd_record_index(bytes: &[u8], offsets: &[usize], entry_idx: usize) -> u32 {
    offsets[..entry_idx].iter().map(|e| bytes[*e + 17] as u32 + 1).sum()
}

/// Compute the record index of the next slot after entry at `entry_idx`.
pub fn scd_next_record_after(bytes: &[u8], offsets: &[usize], entry_idx: usize) -> u32 {
    offsets[..=entry_idx].iter().map(|e| bytes[*e + 17] as u32 + 1).sum()
}
