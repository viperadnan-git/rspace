//! `Workspace` behavior and rendering, split by concern.

use super::*;

mod layout;
pub(crate) mod modal;
mod dock;
mod navigation;
pub(crate) mod pane;
mod remotes;
mod render;
mod settings;
mod split;
mod tabs;
mod toasts;
