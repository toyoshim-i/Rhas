//! ユーティリティ関数の統一モジュール
//!
//! 複数箇所で重複している以下の処理を統一：
//! - バイト列の文字列化
//! - Vec の初期化パターン
//! - 小文字化処理

use std::path::PathBuf;

/// バイト列を文字列に変換（UTF-8 ロス警告対応）
///
/// `String::from_utf8_lossy()` の thin wrapper。
/// 使用例：
/// ```ignore
/// let path = path_from_bytes(b"source.s");
/// ```
pub fn bytes_to_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// バイト列をパス名に変換
///
/// コマンドライン引数やファイル名を `PathBuf` に変換する際に使用。
/// 例：
/// ```ignore
/// let output_path = path_from_bytes(&opts.object_file.unwrap());
/// ```
pub fn path_from_bytes(b: &[u8]) -> PathBuf {
    PathBuf::from(bytes_to_string(b))
}

/// バイト列を小文字化して Vec に変換
///
/// シンボルテーブルのキーや命令名の正規化に使用。
/// 大文字小文字区別なしのハッシュキーに適している。
///
/// 例：
/// ```ignore
/// let key = to_lowercase_vec(b"MOVE");
/// // → b"move".to_vec()
/// ```
pub fn to_lowercase_vec<B: AsRef<[u8]>>(s: B) -> Vec<u8> {
    s.as_ref().iter().map(|c| c.to_ascii_lowercase()).collect()
}

/// バイト列を小文字化（in-place、要求なし）
///
/// `to_lowercase_vec()` と異なり、借用参照から新規割り当てなし。
/// 呼び出し側で直接変更が必要な場合に使用（稀）。
pub fn to_lowercase_buf(s: &mut [u8]) {
    s.iter_mut().for_each(|c| *c = c.to_ascii_lowercase());
}

#[cfg(test)]
mod tests;
