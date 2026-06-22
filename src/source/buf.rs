use crate::error::{FileError, FileErrorKind, SourcePos};
use std::path::PathBuf;

/// 行の最大長（has.equ: MAXLINELEN）
pub const MAX_LINE_LEN: usize = 8 * 1024;

/// 1つのソースファイルのバッファ（全内容）
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
            FileError {
                path: path_bytes,
                kind,
            }
        })?;
        Ok(SourceBuf {
            path,
            data,
            pos: 0,
            line: 0,
        })
    }

    /// インメモリのバイト列からバッファを作成する（テスト・マクロ展開用）
    #[allow(dead_code)]
    pub fn from_bytes(data: Vec<u8>, path: PathBuf) -> Self {
        SourceBuf {
            path,
            data,
            pos: 0,
            line: 0,
        }
    }

    /// ファイル名（パスの最終要素）をバイト列として返す（最大16文字）
    pub fn filename_bytes(&self) -> Vec<u8> {
        let name = self
            .path
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
