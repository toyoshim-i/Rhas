//! rhas エラー・ワーニング処理
//!
//! オリジナルの `error.s` に相当する。
//! エラーコード・ワーニングコードは `errtbl` / `warntbl` マクロ定義から移植。
#![allow(unused_imports)]

pub mod codes;
pub mod context;
pub mod io;
pub mod printer;
pub mod reporter;

pub use codes::{warn, warn_default_level, ErrorCode, WarnCode};
pub use context::{ErrorContext, SourcePos, WarnContext};
pub use io::{FileError, FileErrorKind};
pub use printer::{print_error_context, print_warning_context};
pub use reporter::{ErrorReporter, StderrReporter, BufferReporter, StoredError, StoredWarning};

#[cfg(test)]
mod tests;
