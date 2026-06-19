use crate::utils;
use std::path::PathBuf;

/// `Options::include_paths_cmd` (NUL区切りバイト列) を PathBuf のリストに変換する
pub fn parse_include_paths(raw: Option<&Vec<u8>>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if let Some(data) = raw {
        for part in data.split(|&b| b == 0) {
            if !part.is_empty() {
                let s = utils::bytes_to_string(part);
                result.push(PathBuf::from(&s));
            }
        }
    }
    result
}
