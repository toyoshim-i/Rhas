/// ソースファイル読み込み
///
/// オリジナルの `file.s`（readline, readopen）に相当する。
/// Rust版はファイルを全てメモリに読み込み、イテレータで行を返す方式を採用。
/// インクルードファイルのネストは最大 INCLDMAXNEST(8) レベル。

use crate::error::{FileError, FileErrorKind, SourcePos};
use std::path::{Path, PathBuf};

/// インクルードネスト上限（has.equ: INCLDMAXNEST）
pub const INCLD_MAX_NEST: usize = 8;

/// 行の最大長（has.equ: MAXLINELEN）
pub const MAX_LINE_LEN: usize = 8 * 1024;

// ----------------------------------------------------------------
// ファイルバッファ
// ----------------------------------------------------------------

/// 1つのソースファイルのバッファ（全内容）
///
/// オリジナルでは F_PTR 構造体 + OS ファイルバッファを使っていたが、
/// Rust版はファイル全体を Vec<u8> に読み込んで使う。
#[derive(Debug)]
pub struct SourceBuf {
    /// ファイルパス（エラーメッセージ用）
    pub path: PathBuf,
    /// ファイル内容（全バイト列）
    pub data: Vec<u8>,
    /// 現在の読み込み位置（バイトオフセット）
    pos: usize,
    /// 現在の行番号（1始まり）
    pub line: u32,
}

impl SourceBuf {
    /// ファイルを読み込んでバッファを作成する
    pub fn from_file(path: PathBuf) -> Result<Self, FileError> {
        let data = std::fs::read(&path).map_err(|e| {
            let path_bytes = path.to_string_lossy().as_bytes().to_vec();
            let kind = if e.kind() == std::io::ErrorKind::NotFound {
                FileErrorKind::NotFound
            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                FileErrorKind::AccessDenied
            } else {
                FileErrorKind::ReadError(e)
            };
            FileError { path: path_bytes, kind }
        })?;
        Ok(SourceBuf {
            path,
            data,
            pos: 0,
            line: 0,
        })
    }

    /// インメモリのバイト列からバッファを作成する（テスト・マクロ展開用）
    pub fn from_bytes(data: Vec<u8>, path: PathBuf) -> Self {
        SourceBuf { path, data, pos: 0, line: 0 }
    }

    /// ファイル名（パスの最終要素）をバイト列として返す（最大16文字）
    pub fn filename_bytes(&self) -> Vec<u8> {
        let name = self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        name.into_bytes()
    }

    /// SourcePos を生成する
    pub fn source_pos(&self) -> SourcePos {
        SourcePos::new(self.filename_bytes(), self.line)
    }

    /// EOF かどうか
    pub fn is_eof(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// 1行読み込む（CRLF/LF/CR すべて対応）
    ///
    /// オリジナルの `readline` ルーチンに対応。
    /// 行末の改行は含まない。
    /// EOF に達したら None を返す。
    /// 行が MAX_LINE_LEN を超えた場合でも、超過分を切り捨てて返す。
    pub fn read_line(&mut self) -> Option<Vec<u8>> {
        if self.is_eof() {
            return None;
        }

        self.line += 1;
        let mut line = Vec::with_capacity(128);
        let mut truncated = false;

        loop {
            if self.pos >= self.data.len() {
                break;
            }
            let b = self.data[self.pos];
            self.pos += 1;

            match b {
                b'\r' => {
                    // CR: 次が LF なら読み飛ばす (CRLF → LF)
                    if self.data.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    break;
                }
                b'\n' => {
                    break;
                }
                _ => {
                    if line.len() < MAX_LINE_LEN {
                        line.push(b);
                    } else {
                        truncated = true;
                    }
                }
            }
        }

        // 行末の NUL・制御文字は無視（オリジナルの動作に合わせる）
        let _ = truncated;
        Some(line)
    }
}

// ----------------------------------------------------------------
// インクルードスタック
// ----------------------------------------------------------------

/// インクルードスタック管理
///
/// オリジナルの INCLDINF 配列 + INCLDNEST カウンタに対応。
/// スタックの底がメインソースファイル。
#[derive(Debug)]
pub struct SourceStack {
    /// スタック（底=メイン、上=最内側のインクルード）
    stack: Vec<SourceBuf>,
    /// インクルードパス（-i オプションで指定）
    include_paths: Vec<PathBuf>,
}

impl SourceStack {
    /// メインソースファイルでスタックを初期化する
    pub fn new(main_source: SourceBuf, include_paths: Vec<PathBuf>) -> Self {
        SourceStack {
            stack: vec![main_source],
            include_paths,
        }
    }

    /// 現在のネスト深度
    pub fn nest_depth(&self) -> usize {
        self.stack.len()
    }

    /// 現在処理中のソースバッファへの参照
    pub fn current(&self) -> &SourceBuf {
        self.stack.last().expect("source stack is empty")
    }

    /// 現在処理中のソースバッファへの可変参照
    pub fn current_mut(&mut self) -> &mut SourceBuf {
        self.stack.last_mut().expect("source stack is empty")
    }

    /// 現在位置の SourcePos を返す
    pub fn source_pos(&self) -> SourcePos {
        self.current().source_pos()
    }

    /// 1行読み込む。
    /// 現在のファイルが EOF なら、スタックを巻き戻して上位ファイルから読む。
    /// 全て EOF ならば None を返す。
    pub fn read_line(&mut self) -> ReadResult {
        loop {
            let is_eof = self.stack.last().map(|s| s.is_eof()).unwrap_or(true);
            if !is_eof {
                let line = self.current_mut().read_line();
                return match line {
                    Some(l) => ReadResult::Line(l),
                    None => ReadResult::Eof,
                };
            }
            // 現在ファイルが EOF
            if self.stack.len() == 1 {
                // メインファイルの EOF = アセンブル終了
                return ReadResult::Eof;
            }
            // インクルードファイルの EOF = スタックを1段戻す
            self.stack.pop();
            // インクルード終了イベント（Pass1 で T_INCLDEND を記録するため）
            return ReadResult::IncludeEnd;
        }
    }

    /// インクルードファイルをオープンしてスタックに積む
    ///
    /// オリジナルの .include 疑似命令処理に対応。
    pub fn push_include(&mut self, filename: &[u8]) -> Result<(), FileError> {
        if self.stack.len() >= INCLD_MAX_NEST {
            // ネスト上限に達した場合は呼び出し元で ErrorCode::TooIncld を発行する
            return Err(FileError {
                path: filename.to_vec(),
                kind: FileErrorKind::NotFound, // ここでは仮; 呼び出し側で TooIncld に変換
            });
        }

        let path = self.resolve_include_path(filename)?;
        let buf = SourceBuf::from_file(path)?;
        self.stack.push(buf);
        Ok(())
    }

    /// インクルードパスを検索してファイルパスを解決する
    ///
    /// 1. カレントファイルと同じディレクトリを先に探す
    /// 2. -i で指定したパスを順番に探す
    fn resolve_include_path(&self, filename: &[u8]) -> Result<PathBuf, FileError> {
        let name_str = String::from_utf8_lossy(filename);
        let name_path = Path::new(name_str.as_ref());

        // 絶対パスならそのまま
        if name_path.is_absolute() {
            if name_path.exists() {
                return Ok(name_path.to_path_buf());
            }
            return Err(FileError {
                path: filename.to_vec(),
                kind: FileErrorKind::NotFound,
            });
        }

        // カレントファイルと同じディレクトリ
        let cur_dir = self.current().path.parent().unwrap_or(Path::new("."));
        let candidate = cur_dir.join(name_path);
        if candidate.exists() {
            return Ok(candidate);
        }

        // -i パスを順に検索
        for inc_dir in &self.include_paths {
            let candidate = inc_dir.join(name_path);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        Err(FileError {
            path: filename.to_vec(),
            kind: FileErrorKind::NotFound,
        })
    }
}

/// 1行読み込みの結果
#[derive(Debug)]
pub enum ReadResult {
    /// 行データ
    Line(Vec<u8>),
    /// ファイル終端（メインファイルの EOF、またはインクルード終端から戻った後）
    Eof,
    /// インクルードファイルの終端（呼び出し元で T_INCLDEND を記録する）
    IncludeEnd,
}

// ----------------------------------------------------------------
// インクルードパスの解析ユーティリティ
// ----------------------------------------------------------------

/// `Options::include_paths_cmd` (NUL区切りバイト列) を PathBuf のリストに変換する
pub fn parse_include_paths(raw: Option<&Vec<u8>>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(data) = raw {
        for part in data.split(|&b| b == 0) {
            if !part.is_empty() {
                let s = String::from_utf8_lossy(part);
                result.push(PathBuf::from(s.as_ref()));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buf(data: &[u8]) -> SourceBuf {
        SourceBuf::from_bytes(data.to_vec(), PathBuf::from("test.s"))
    }

    #[test]
    fn test_read_line_lf() {
        let mut buf = make_buf(b"line1\nline2\n");
        assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
        assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
        assert_eq!(buf.read_line(), None);
    }

    #[test]
    fn test_read_line_crlf() {
        let mut buf = make_buf(b"line1\r\nline2\r\n");
        assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
        assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
        assert_eq!(buf.read_line(), None);
    }

    #[test]
    fn test_read_line_cr_only() {
        let mut buf = make_buf(b"line1\rline2\r");
        assert_eq!(buf.read_line(), Some(b"line1".to_vec()));
        assert_eq!(buf.read_line(), Some(b"line2".to_vec()));
        assert_eq!(buf.read_line(), None);
    }

    #[test]
    fn test_read_line_no_trailing_newline() {
        let mut buf = make_buf(b"only");
        assert_eq!(buf.read_line(), Some(b"only".to_vec()));
        assert_eq!(buf.read_line(), None);
    }

    #[test]
    fn test_line_number_tracking() {
        let mut buf = make_buf(b"a\nb\nc\n");
        buf.read_line();
        assert_eq!(buf.line, 1);
        buf.read_line();
        assert_eq!(buf.line, 2);
        buf.read_line();
        assert_eq!(buf.line, 3);
    }

    #[test]
    fn test_include_paths_parse() {
        let raw = b"path/a\0path/b\0".to_vec();
        let paths = parse_include_paths(Some(&raw));
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("path/a"));
        assert_eq!(paths[1], PathBuf::from("path/b"));
    }
}
