//! Pseudo-instruction handlers
//!
//! This module provides modular handlers for various pseudo-instructions (directives)
//! that were previously embedded in pass1.rs handle_pseudo().
//!
//! Organization by category:
//! - section: Section switching directives (.text, .data, etc.)
//! - data: Data definition directives (.dc, .ds, .dcb)
//! - conditional: Conditional assembly (.if, .ifdef, .else, .endif)
//! - macro: Macro and repetition (.macro, .rept, .irp)
//! - debug: SCD debugging directives
//! - misc: Remaining directives (.org, .fail, .cpu, etc.)

pub mod section;
pub mod data;
// TODO: pub mod conditional;
// TODO: pub mod macro_;
// TODO: pub mod debug;
// TODO: pub mod misc;
