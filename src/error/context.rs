#![allow(dead_code)]

use super::codes::{ErrorCode, WarnCode};
use crate::utils;

/// ソースコード上の位置（ファイル名+行番号）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePos {
    /// ファイル名（バイト列、最大16文字表示）
    pub filename: Vec<u8>,
    /// 行番号（1始まり）
    pub line: u32,
}

impl SourcePos {
    pub fn new(filename: Vec<u8>, line: u32) -> Self {
        SourcePos { filename, line }
    }

    /// ファイル名を表示用文字列として取得（最大16文字）
    pub fn filename_display(&self) -> String {
        let s = utils::bytes_to_string(&self.filename);
        if s.len() <= 16 {
            format!("{:<16}", s)
        } else {
            s[..16].to_string()
        }
    }
}

/// エラーレポート用の構造化コンテキスト
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorContext {
    /// ソースコード位置
    pub pos: SourcePos,
    /// エラーコード
    pub code: ErrorCode,
    /// 関連シンボル（オプション）
    pub symbol: Option<Vec<u8>>,
}

impl ErrorContext {
    /// 新しい ErrorContext を生成
    pub fn new(pos: SourcePos, code: ErrorCode, symbol: Option<Vec<u8>>) -> Self {
        ErrorContext { pos, code, symbol }
    }

    /// ErrorContext をシンボル付きで生成（&[u8] から）
    pub fn with_symbol(pos: SourcePos, code: ErrorCode, symbol: &[u8]) -> Self {
        ErrorContext {
            pos,
            code,
            symbol: Some(symbol.to_vec()),
        }
    }
}

/// ワーニングレポート用の構造化コンテキスト
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarnContext {
    /// ソースコード位置
    pub pos: SourcePos,
    /// ワーニングコード
    pub code: WarnCode,
    /// 関連シンボル（オプション）
    pub symbol: Option<Vec<u8>>,
}

impl WarnContext {
    /// 新しい WarnContext を生成
    pub fn new(pos: SourcePos, code: WarnCode, symbol: Option<Vec<u8>>) -> Self {
        WarnContext { pos, code, symbol }
    }

    /// WarnContext をシンボル付きで生成（&[u8] から）
    pub fn with_symbol(pos: SourcePos, code: WarnCode, symbol: &[u8]) -> Self {
        WarnContext {
            pos,
            code,
            symbol: Some(symbol.to_vec()),
        }
    }
}
