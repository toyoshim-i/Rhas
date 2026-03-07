//! Pseudo-instruction handlers for section directives
//!
//! Handles: .text, .data, .bss, .stack, .rdata, .rbss, .rstack, .rldata, .rlbss, .rlstack
//! These directives switch between different code/data sections.

use crate::context::{AssemblyContext, Section};
use crate::symbol::types::InsnHandler;
use super::super::temp::TempRecord;

/// Handle section directive (.text, .data, etc.)
///
/// Maps InsnHandler to corresponding Section and generates SectChange record.
pub fn handle_section(
    handler: InsnHandler,
    ctx: &mut AssemblyContext,
    records: &mut Vec<TempRecord>,
) {
    let (section, sect_id) = match handler {
        InsnHandler::TextSect => (Section::Text, 0x01u8),
        InsnHandler::DataSect => (Section::Data, 0x02u8),
        InsnHandler::BssSect => (Section::Bss, 0x03u8),
        InsnHandler::Stack => (Section::Stack, 0x04u8),
        InsnHandler::RdataSect => (Section::Rdata, 0x05u8),
        InsnHandler::RbssSect => (Section::Rbss, 0x06u8),
        InsnHandler::RstackSect => (Section::Rstack, 0x07u8),
        InsnHandler::RldataSect => (Section::Rldata, 0x08u8),
        InsnHandler::RlbssSect => (Section::Rlbss, 0x09u8),
        InsnHandler::RlstackSect => (Section::Rlstack, 0x0Au8),
        _ => return, // Not a section directive
    };

    ctx.set_section(section);
    records.push(TempRecord::SectChange { id: sect_id });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_section_text() {
        let mut ctx = AssemblyContext::new(crate::options::Options::default());
        let mut records = Vec::new();

        handle_section(InsnHandler::TextSect, &mut ctx, &mut records);

        assert_eq!(ctx.section, Section::Text);
        assert_eq!(records.len(), 1);
        if let TempRecord::SectChange { id } = &records[0] {
            assert_eq!(*id, 0x01u8);
        } else {
            panic!("Expected SectChange record");
        }
    }

    #[test]
    fn test_handle_section_data() {
        let mut ctx = AssemblyContext::new(crate::options::Options::default());
        let mut records = Vec::new();

        handle_section(InsnHandler::DataSect, &mut ctx, &mut records);

        assert_eq!(ctx.section, Section::Data);
    }
}
