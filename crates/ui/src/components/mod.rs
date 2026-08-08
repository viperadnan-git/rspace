//! Reusable, domain-agnostic UI components (widgets, inputs, modals, toasts).
//!
//! `use super::*` re-exposes the crate root so components written against it
//! (via their own `use super::*`) resolve `Workspace`, widgets, and gpui types.

use super::*;

pub mod confirm;
pub mod number_field;
pub mod picker;
pub mod prompt;
pub mod text_input;
pub mod toast;
pub mod update_modal;
pub mod widgets;
