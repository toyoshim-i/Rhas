//! Pseudo-instruction handlers for data directives
//!
//! Handles: .dc (declare constant), .ds (declare space), .dcb (declare block)
//! These directives define data in the current section.

use crate::symbol::types::InsnHandler;
use crate::symbol::types::SizeCode;
use super::super::temp::TempRecord;

/// Size code to byte count conversion
fn size_to_bytes(size: Option<SizeCode>) -> u32 {
    match size {
        Some(SizeCode::Byte) => 1,
        Some(SizeCode::Long) => 4,
        None | Some(SizeCode::Word) => 2,
        _ => 2,
    }
}

/// Handle data definition directive (.dc, .ds, .dcb)
///
/// Parameters:
/// - handler: InsnHandler type (Dc, Ds, or Dcb)
/// - size: Optional size code for the data element
/// - line: Assembly source line (parsed elsewhere for data content)
/// - pos: Current position in line
/// - is_offset_mode: Whether in .offset mode (skips actual data output)
///
/// Note: This is a stub that will be integrated with parse_dc and eval_const
/// from pass1 context. Full implementation requires pass1 infrastructure.
pub fn handle_data(
    handler: InsnHandler,
    size: Option<SizeCode>,
) -> Option<u32> {
    // This returns the size code conversion for now
    // Full implementation will be done during pass1 integration
    match handler {
        InsnHandler::Dc | InsnHandler::Ds | InsnHandler::Dcb => {
            Some(size_to_bytes(size))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_dc_byte() {
        let size = handle_data(InsnHandler::Dc, Some(SizeCode::Byte));
        assert_eq!(size, Some(1));
    }

    #[test]
    fn test_data_dc_word() {
        let size = handle_data(InsnHandler::Dc, None);
        assert_eq!(size, Some(2));
    }

    #[test]
    fn test_data_dc_long() {
        let size = handle_data(InsnHandler::Dc, Some(SizeCode::Long));
        assert_eq!(size, Some(4));
    }

    #[test]
    fn test_data_ds_byte() {
        let size = handle_data(InsnHandler::Ds, Some(SizeCode::Byte));
        assert_eq!(size, Some(1));
    }

    #[test]
    fn test_data_dcb_long() {
        let size = handle_data(InsnHandler::Dcb, Some(SizeCode::Long));
        assert_eq!(size, Some(4));
    }
}
