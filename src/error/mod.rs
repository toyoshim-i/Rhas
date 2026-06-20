//! rhas エラー・ワーニング処理
//!
//! オリジナルの `error.s` に相当する。
//! エラーコード・ワーニングコードは `errtbl` / `warntbl` マクロ定義から移植。

pub mod codes;
pub mod context;
pub mod io;
pub mod printer;

pub use codes::{warn, warn_default_level, warn_message, ErrorCode, WarnCode};
pub use context::{ErrorContext, SourcePos, WarnContext};
pub use io::{FileError, FileErrorKind};
pub use printer::{print_error, print_error_context, print_warning, print_warning_context};

#[cfg(test)]
mod tests;
