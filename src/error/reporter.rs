#![allow(dead_code)]
use std::io::Write;
use crate::error::codes::{ErrorCode, WarnCode, warn_default_level};
use crate::error::context::SourcePos;

/// エラーと警告の報告を行うトレイト
pub trait ErrorReporter {
    /// エラーを報告する
    fn report_error(&mut self, pos: &SourcePos, code: ErrorCode, symbol: Option<&[u8]>);
    /// 警告を報告する
    fn report_warning(&mut self, pos: &SourcePos, code: WarnCode, symbol: Option<&[u8]>);
    /// 報告されたエラーの総数を返す
    fn error_count(&self) -> u32;
    /// 報告された警告の総数を返す
    fn warning_count(&self) -> u32;
}

/// 標準エラー（または任意の Write 構造体）に出力するレポーター
pub struct StderrReporter<W: Write> {
    out: W,
    warn_level: u8,
    error_count: u32,
    warning_count: u32,
}

impl StderrReporter<std::io::Stderr> {
    /// 標準エラー出力用の新規レポーターを作成する
    pub fn new(warn_level: u8) -> Self {
        StderrReporter {
            out: std::io::stderr(),
            warn_level,
            error_count: 0,
            warning_count: 0,
        }
    }
}

impl<W: Write> StderrReporter<W> {
    /// 任意の出力先をターゲットにするレポーターを作成する（テスト用など）
    pub fn with_writer(out: W, warn_level: u8) -> Self {
        StderrReporter {
            out,
            warn_level,
            error_count: 0,
            warning_count: 0,
        }
    }
}

impl<W: Write> ErrorReporter for StderrReporter<W> {
    fn report_error(&mut self, pos: &SourcePos, code: ErrorCode, symbol: Option<&[u8]>) {
        super::printer::print_error(&mut self.out, pos, code, symbol);
        self.error_count += 1;
    }

    fn report_warning(&mut self, pos: &SourcePos, code: WarnCode, symbol: Option<&[u8]>) {
        super::printer::print_warning(&mut self.out, pos, code, symbol, self.warn_level);
        if self.warn_level >= warn_default_level(code) {
            self.warning_count += 1;
        }
    }

    fn error_count(&self) -> u32 {
        self.error_count
    }

    fn warning_count(&self) -> u32 {
        self.warning_count
    }
}

/// メモリ上にエラー/警告を蓄積する構造体
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredError {
    pub pos: SourcePos,
    pub code: ErrorCode,
    pub symbol: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWarning {
    pub pos: SourcePos,
    pub code: WarnCode,
    pub symbol: Option<Vec<u8>>,
}

/// メモリバッファに蓄積するテスト検証用レポーター
pub struct BufferReporter {
    pub errors: Vec<StoredError>,
    pub warnings: Vec<StoredWarning>,
    warn_level: u8,
}

impl BufferReporter {
    /// バッファレポーターを作成する
    pub fn new(warn_level: u8) -> Self {
        BufferReporter {
            errors: Vec::new(),
            warnings: Vec::new(),
            warn_level,
        }
    }
}

impl ErrorReporter for BufferReporter {
    fn report_error(&mut self, pos: &SourcePos, code: ErrorCode, symbol: Option<&[u8]>) {
        self.errors.push(StoredError {
            pos: pos.clone(),
            code,
            symbol: symbol.map(|s| s.to_vec()),
        });
    }

    fn report_warning(&mut self, pos: &SourcePos, code: WarnCode, symbol: Option<&[u8]>) {
        if self.warn_level >= warn_default_level(code) {
            self.warnings.push(StoredWarning {
                pos: pos.clone(),
                code,
                symbol: symbol.map(|s| s.to_vec()),
            });
        }
    }

    fn error_count(&self) -> u32 {
        self.errors.len() as u32
    }

    fn warning_count(&self) -> u32 {
        self.warnings.len() as u32
    }
}
