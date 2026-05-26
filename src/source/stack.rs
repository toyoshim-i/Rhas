use std::path::{Path, PathBuf};
use crate::error::{FileError, FileErrorKind, SourcePos};
use crate::utils;
use super::buf::SourceBuf;

/// インクルードネスト上限（has.equ: INCLDMAXNEST）
pub const INCLD_MAX_NEST: usize = 8;

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
    ///
    /// # Panics
    ///
    /// スタックが空の場合にパニックする。
    /// このスタックは常にメインソースファイルを含むため、通常は起こり得ない。
    pub fn current(&self) -> &SourceBuf {
        self.stack.last().expect("source stack is empty")
    }

    /// 現在処理中のソースバッファへの可変参照
    ///
    /// # Panics
    ///
    /// スタックが空の場合にパニックする。
    /// このスタックは常にメインソースファイルを含むため、通常は起こり得ない。
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
        ReadResult::IncludeEnd
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
        let name_str = utils::bytes_to_string(filename);
        let name_path = Path::new(&name_str);

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
