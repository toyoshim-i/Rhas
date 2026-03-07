//! Pseudo-instruction handlers for miscellaneous directives
//!
//! Handles: .org, .fail, .cpu, .globl, .extern, .comm, .even, .align, etc.
//! These are less complex directives not covered by other modules.

/// CPU type specification support
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuDirective {
    /// .cpu 68000
    Cpu68000 = 68000,
    /// .cpu 68010
    Cpu68010 = 68010,
    /// .cpu 68020
    Cpu68020 = 68020,
    /// .cpu 68030
    Cpu68030 = 68030,
    /// .cpu 68040
    Cpu68040 = 68040,
    /// .cpu 68060
    Cpu68060 = 68060,
}

impl CpuDirective {
    /// Parse CPU number from input
    pub fn from_number(n: u32) -> Option<Self> {
        match n {
            68000 => Some(CpuDirective::Cpu68000),
            68010 => Some(CpuDirective::Cpu68010),
            68020 => Some(CpuDirective::Cpu68020),
            68030 => Some(CpuDirective::Cpu68030),
            68040 => Some(CpuDirective::Cpu68040),
            68060 => Some(CpuDirective::Cpu68060),
            _ => None,
        }
    }

    /// Get CPU number value
    pub fn number(&self) -> u32 {
        *self as u32
    }

    /// Check if CPU supports instruction
    pub fn supports_fpu(&self) -> bool {
        self.number() >= 68040
    }
}

/// Alignment specifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignmentOperand {
    /// 2-byte alignment (.even)
    Even = 2,
    /// 4-byte alignment
    Quad = 4,
    /// 8-byte alignment
    Octa = 8,
    /// 16-byte alignment
    Hex = 16,
}

impl AlignmentOperand {
    /// Get alignment boundary size
    pub fn boundary(&self) -> u32 {
        *self as u32
    }
}

/// Helper to parse .org argument
pub fn parse_org_address(value: u32) -> u32 {
    value
}

/// Helper to validate symbol visibility
pub fn is_visibility_directive(name: &[u8]) -> bool {
    matches!(name, b"globl" | b"GLOBL" | b"extern" | b"EXTERN")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_directive_from_number() {
        assert_eq!(CpuDirective::from_number(68000), Some(CpuDirective::Cpu68000));
        assert_eq!(CpuDirective::from_number(68020), Some(CpuDirective::Cpu68020));
        assert_eq!(CpuDirective::from_number(68060), Some(CpuDirective::Cpu68060));
        assert_eq!(CpuDirective::from_number(99999), None);
    }

    #[test]
    fn test_cpu_directive_number() {
        assert_eq!(CpuDirective::Cpu68000.number(), 68000);
        assert_eq!(CpuDirective::Cpu68060.number(), 68060);
    }

    #[test]
    fn test_cpu_supports_fpu() {
        assert!(!CpuDirective::Cpu68020.supports_fpu());
        assert!(CpuDirective::Cpu68040.supports_fpu());
        assert!(CpuDirective::Cpu68060.supports_fpu());
    }

    #[test]
    fn test_alignment_operand_boundary() {
        assert_eq!(AlignmentOperand::Even.boundary(), 2);
        assert_eq!(AlignmentOperand::Quad.boundary(), 4);
        assert_eq!(AlignmentOperand::Octa.boundary(), 8);
        assert_eq!(AlignmentOperand::Hex.boundary(), 16);
    }

    #[test]
    fn test_parse_org_address() {
        assert_eq!(parse_org_address(0x1000), 0x1000);
        assert_eq!(parse_org_address(0), 0);
    }

    #[test]
    fn test_is_visibility_directive() {
        assert!(is_visibility_directive(b"globl"));
        assert!(is_visibility_directive(b"extern"));
        assert!(!is_visibility_directive(b"label"));
        assert!(!is_visibility_directive(b"text"));
    }
}
