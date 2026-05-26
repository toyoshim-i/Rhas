use crate::utils;

/// ファイル操作エラー（アセンブル中のI/Oエラー）
#[derive(Debug)]
pub struct FileError {
    pub path: Vec<u8>,
    pub kind: FileErrorKind,
}

#[derive(Debug)]
pub enum FileErrorKind {
    NotFound,
    AccessDenied,
    ReadError(std::io::Error),
    WriteError(std::io::Error),
    CreateError(std::io::Error),
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = utils::bytes_to_string(&self.path);
        match &self.kind {
            FileErrorKind::NotFound => write!(f, "ファイルが見つかりません: {}", path),
            FileErrorKind::AccessDenied => write!(f, "アクセス拒否: {}", path),
            FileErrorKind::ReadError(e) => write!(f, "読み込みエラー: {} ({})", path, e),
            FileErrorKind::WriteError(e) => write!(f, "書き込みエラー: {} ({})", path, e),
            FileErrorKind::CreateError(e) => write!(f, "作成エラー: {} ({})", path, e),
        }
    }
}
