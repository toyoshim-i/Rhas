//! パスシステム（Pass 1 → Pass 2 → Pass 3）
//!
//! オリジナルの 3 パスアセンブラ構造に対応する。
//! Rust版はテンポラリファイルの代わりにメモリ上の Vec<TempRecord> を使う。

pub mod temp;
pub mod pass1;
pub mod pass2;
pub mod pass3;
pub mod prn;

use crate::context::AssemblyContext;
use crate::context::AsmPass;
use crate::object::writer::write_hlk;
use crate::object::ObjectCode;
use crate::source::{parse_include_paths, SourceBuf, SourceStack};
use crate::symbol::{Symbol, SymbolTable};
use crate::symbol::types::{DefAttrib, ExtAttrib};
use std::path::PathBuf;

/// シンボルファイルをバイト列として生成する（-x オプション）
fn format_sym_file(sym: &SymbolTable) -> Vec<u8> {
    let mut out = Vec::new();
    let mut entries: Vec<(&Vec<u8>, &Symbol)> = sym.iter_user_syms().collect();
    // シンボル名でソート
    entries.sort_by_key(|(name, _)| name.as_slice());

    for (name, symbol) in &entries {
        // シンボル名（16文字、スペースパディング）
        let name_len = name.len().min(16);
        out.extend_from_slice(&name[..name_len]);
        out.extend(std::iter::repeat_n(b' ', 16 - name_len));
        out.extend_from_slice(b" : ");

        match symbol {
            Symbol::Value { value, section, attrib, ext_attrib, .. } => {
                let is_common = matches!(ext_attrib, ExtAttrib::Comm | ExtAttrib::RComm | ExtAttrib::RLComm);
                if *attrib < DefAttrib::Define && !is_common {
                    out.extend_from_slice(b"UNDEF           \n");
                    continue;
                }
                // セクション種別
                let sect_str: &[u8] = match ext_attrib {
                    ExtAttrib::XRef => b"XREF ",
                    ExtAttrib::Comm => b"COMM ",
                    ExtAttrib::RComm => b"RCOM ",
                    ExtAttrib::RLComm => b"RLCM ",
                    ExtAttrib::Globl => b"GLOB ",
                    _ => match *section {
                        0x00 => b"ABS  ",
                        0x01 => b"TEXT ",
                        0x02 => b"DATA ",
                        0x03 => b"BSS  ",
                        0x04 => b"STCK ",
                        _    => b"     ",
                    },
                };
                out.extend_from_slice(sect_str);
                let v = format!("{:08X}", *value as u32);
                out.extend_from_slice(v.as_bytes());
                out.push(b'\n');
            }
            Symbol::Macro { .. } => {
                out.extend_from_slice(b"MACRO\n");
            }
            _ => {
                out.extend_from_slice(b"OTHER\n");
            }
        }
    }
    out
}

/// アセンブルエラー
#[derive(Debug)]
pub enum AssembleError {
    /// ソースファイルが開けない
    SourceNotFound(PathBuf),
    /// アセンブルエラーあり（エラー数）
    HasErrors(u32),
    /// IO エラー
    Io(std::io::Error),
}

/// アセンブル結果
pub struct AssembleResult {
    pub obj_bytes: Vec<u8>,
    pub obj: ObjectCode,
    pub num_errors: u32,
    pub num_warnings: u32,
}

/// メインのアセンブルエントリポイント
///
/// ソースファイルを読み込み、3 パス処理を行い、HLK オブジェクトバイト列を返す。
pub fn assemble(ctx: &mut AssemblyContext) -> Result<AssembleResult, AssembleError> {
    // ---- ソースファイル準備 ----
    let source_path = ctx.opts.source_file.as_deref()
        .map(|b| PathBuf::from(String::from_utf8_lossy(b).as_ref()))
        .unwrap_or_else(|| PathBuf::from("(stdin)"));

    let source_buf = SourceBuf::from_file(source_path.clone())
        .map_err(|_| AssembleError::SourceNotFound(source_path.clone()))?;

    // ソース名（拡張子なし）を取得（$D000 ヘッダ用）
    let source_name: Vec<u8> = source_path
        .file_stem()
        .map(|s| s.to_string_lossy().as_bytes().to_vec())
        .unwrap_or_else(|| b"unknown".to_vec());
    // ソースファイル名（拡張子あり）を取得（$B204 レコード用）
    let source_file: Vec<u8> = source_path
        .file_name()
        .map(|s| s.to_string_lossy().as_bytes().to_vec())
        .unwrap_or_else(|| source_name.clone());

    let include_paths = parse_include_paths(ctx.opts.include_paths_cmd.as_ref());
    let mut source = SourceStack::new(source_buf, include_paths);

    // ---- シンボルテーブル初期化 ----
    let mut sym = SymbolTable::new(ctx.opts.sym_len8);

    // ---- Pass 1: ソース解析 → TempRecord ----
    ctx.pass = AsmPass::Pass1;
    let mut records = pass1::pass1(&mut source, ctx, &mut sym);

    // ---- Pass 2: ロケーション再計算 ----
    ctx.pass = AsmPass::Pass2;
    pass2::pass2(&mut records, &mut sym);

    // ---- Pass 3: コード生成 → ObjectCode ----
    ctx.pass = AsmPass::Pass3;
    let prn_enable = ctx.opts.make_prn;
    let max_align = ctx.max_align;
    let (mut obj, prn_lines) = pass3::pass3(&records, &sym, source_name.clone(), source_file.clone(), prn_enable, max_align);
    obj.has_debug_info = ctx.opts.make_sym_deb;
    obj.scd_enabled = ctx.scd_enabled;
    // HAS互換:
    // -g モードでは SCDフッタの `.file` は入力ソース名を使う。
    // SCD疑似命令モード（.file 有効）では `.file` 指定名を使う。
    obj.scd_file = if ctx.scd_enabled && !ctx.opts.make_sym_deb {
        ctx.scd_file.clone()
    } else {
        source_file
    };
    obj.request_files = ctx.request_files.clone();

    // ---- HLK バイナリ生成 ----
    let obj_bytes = write_hlk(&obj);

    // ---- PRNリストファイル生成 ----
    if ctx.opts.make_prn && !prn_lines.is_empty() {
        let prn_bytes = prn::format_prn(
            &prn_lines,
            &ctx.prn_title,
            &ctx.prn_subttl,
            ctx.opts.prn_width as usize,
            ctx.opts.prn_code_width as usize,
            ctx.opts.prn_no_page_ff,
            ctx.opts.prn_page_lines as usize,
        );
        let prn_path = if let Some(ref p) = ctx.opts.prn_file {
            PathBuf::from(String::from_utf8_lossy(p).as_ref())
        } else {
            // ソースファイルの拡張子を .prn に変換
            let src = PathBuf::from(String::from_utf8_lossy(
                ctx.opts.source_file.as_deref().unwrap_or(b"unknown")
            ).as_ref());
            src.with_extension("prn")
        };
        if let Err(e) = std::fs::write(&prn_path, &prn_bytes) {
            let _ = e; // PRNファイル書き出し失敗は無視
        }
    }

    // ---- シンボルファイル生成 ----
    if ctx.opts.make_sym {
        let sym_bytes = format_sym_file(&sym);
        let sym_path = if let Some(ref p) = ctx.opts.sym_file {
            PathBuf::from(String::from_utf8_lossy(p).as_ref())
        } else {
            // ソースファイルの拡張子を .sym に変換
            let src = PathBuf::from(String::from_utf8_lossy(
                ctx.opts.source_file.as_deref().unwrap_or(b"unknown")
            ).as_ref());
            src.with_extension("sym")
        };
        if let Err(e) = std::fs::write(&sym_path, &sym_bytes) {
            let _ = e; // シンボルファイル書き出し失敗は無視
        }
    }

    let result = AssembleResult {
        obj_bytes,
        obj,
        num_errors: ctx.num_errors,
        num_warnings: ctx.num_warnings,
    };

    if ctx.num_errors > 0 {
        return Err(AssembleError::HasErrors(ctx.num_errors));
    }

    Ok(result)
}
