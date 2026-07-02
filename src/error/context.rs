use super::codes::{ErrorCode, WarnCode};
use crate::utils;

use std::path::PathBuf;

/// ソースコード上の位置（ファイル名+行番号）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePos {
    /// ファイル名（バイト列、最大16文字表示）
    pub filename: Vec<u8>,
    /// 行番号（1始まり）
    pub line: u32,
    /// ソースファイルのフルパス（オプション、モダンエラー表示用）
    pub filepath: Option<PathBuf>,
}

impl SourcePos {
    pub fn new(filename: Vec<u8>, line: u32) -> Self {
        SourcePos {
            filename,
            line,
            filepath: None,
        }
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

/// エラーレポート用の構造化コンテキスト（参照型）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorContext<'a> {
    /// ソースコード位置
    pub pos: &'a SourcePos,
    /// エラーコード
    pub code: ErrorCode,
    /// 関連シンボル（オプション）
    pub symbol: Option<&'a [u8]>,
}

impl<'a> ErrorContext<'a> {
    /// 新しい ErrorContext を生成
    pub fn new(pos: &'a SourcePos, code: ErrorCode, symbol: Option<&'a [u8]>) -> Self {
        ErrorContext { pos, code, symbol }
    }
}

/// ワーニングレポート用の構造化コンテキスト（参照型）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarnContext<'a> {
    /// ソースコード位置
    pub pos: &'a SourcePos,
    /// ワーニングコード
    pub code: WarnCode,
    /// 関連シンボル（オプション）
    pub symbol: Option<&'a [u8]>,
}

impl<'a> WarnContext<'a> {
    /// 新しい WarnContext を生成
    pub fn new(pos: &'a SourcePos, code: WarnCode, symbol: Option<&'a [u8]>) -> Self {
        WarnContext { pos, code, symbol }
    }
}
