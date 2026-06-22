//! アセンブルコンテキスト（AssemblyContext）
//!
//! オリジナルの `work.s` のワークエリア全体に相当する。

pub mod asm;
pub mod cpu;
pub mod scd;
pub mod section;

pub use asm::{AsmPass, AssemblyContext};
pub use cpu::CpuType;
pub use scd::ScdTemp;
pub use section::Section;

#[cfg(test)]
mod tests;
