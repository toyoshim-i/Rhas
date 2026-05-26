//! アセンブルコンテキスト（AssemblyContext）
//!
//! オリジナルの `work.s` のワークエリア全体に相当する。

pub mod section;
pub mod cpu;
pub mod scd;
pub mod asm;

pub use section::{Section, N_SECTIONS};
pub use cpu::CpuType;
pub use scd::ScdTemp;
pub use asm::{AsmPass, AssemblyContext};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Options;

    fn make_ctx() -> AssemblyContext {
        AssemblyContext::new(Options::default())
    }

    #[test]
    fn test_location_starts_zero() {
        let ctx = make_ctx();
        assert_eq!(ctx.location(), 0);
    }

    #[test]
    fn test_advance_location() {
        let mut ctx = make_ctx();
        ctx.advance_location(4);
        assert_eq!(ctx.location(), 4);
        ctx.advance_location(2);
        assert_eq!(ctx.location(), 6);
    }

    #[test]
    fn test_section_switch() {
        let mut ctx = make_ctx();
        ctx.advance_location(10);
        ctx.set_section(Section::Data);
        assert_eq!(ctx.location(), 0);  // data は別カウンタ
        ctx.advance_location(4);
        ctx.set_section(Section::Text);
        assert_eq!(ctx.location(), 10); // text は保持されている
    }

    #[test]
    fn test_error_count() {
        let mut ctx = make_ctx();
        assert!(ctx.is_success());
        ctx.add_error();
        assert!(!ctx.is_success());
        assert_eq!(ctx.num_errors, 1);
    }

    #[test]
    fn test_cpu_init() {
        let ctx = make_ctx();
        assert_eq!(ctx.cpu.number, 68000);
    }

    #[test]
    fn test_cpu_type_creation() {
        let cpu = CpuType::new(68020, 0x0004);
        assert_eq!(cpu.number, 68020);
        assert_eq!(cpu.features, 0x0004);
    }

    #[test]
    fn test_cpu_type_factories() {
        let cpu_000 = CpuType::default_68000();
        assert_eq!(cpu_000.number, 68000);
        
        let cpu_010 = CpuType::cpu_68010();
        assert_eq!(cpu_010.number, 68010);
        
        let cpu_020 = CpuType::cpu_68020();
        assert_eq!(cpu_020.number, 68020);
    }

    #[test]
    fn test_cpu_type_checks() {
        let cpu_010 = CpuType::cpu_68010();
        assert!(cpu_010.is_older_than_020());
        
        let cpu_020 = CpuType::cpu_68020();
        assert!(!cpu_020.is_older_than_020());
    }

    #[test]
    fn test_set_cpu() {
        let mut ctx = make_ctx();
        let cpu = CpuType::new(68030, 0x0008);
        ctx.set_cpu(cpu);
        
        assert_eq!(ctx.cpu.number, 68030);
        assert_eq!(ctx.cpu.features, 0x0008);
    }

    #[test]
    fn test_get_cpu_type() {
        let mut ctx = make_ctx();
        ctx.set_cpu(CpuType::new(68040, 0x0010));
        
        let cpu = ctx.get_cpu_type();
        assert_eq!(cpu.number, 68040);
        assert_eq!(cpu.features, 0x0010);
    }
}
