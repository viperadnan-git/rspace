//! GitHub Dark Dimmed palette (Zed theme) and layout constants.

pub const CANVAS: u32 = 0x212830;
pub const INSET: u32 = 0x151b23;
pub const ELEVATED: u32 = 0x2a313c;
pub const BORDER_MUTED: u32 = 0x3d444d;
pub const FG: u32 = 0xd1d7e0;
pub const FG_MUTED: u32 = 0x9198a1;
pub const FG_SUBTLE: u32 = 0x656c76;
pub const ACCENT: u32 = 0x478be6;
pub const SUCCESS: u32 = 0x57ab5a;
pub const DANGER: u32 = 0xe5534b;
// Translucent element overlays, neutral over either pane background.
pub const OVERLAY: u32 = 0x656c7626;
pub const SELECT: u32 = 0x656c7659;
pub const SELECT_MUTED: u32 = 0x656c7633;

pub const SIDEBAR_W: f32 = 248.0;
pub const SIDEBAR_MIN: f32 = 180.0;
pub const SIDEBAR_MAX: f32 = 480.0;
pub const RESIZE_HANDLE_W: f32 = 6.0;
pub const TITLE_BAR_H: f32 = 36.0;
pub const MAX_CRUMBS: usize = 4;

/// Left inset of the custom title bar to clear the window controls.
#[cfg(target_os = "macos")]
pub const TITLE_BAR_LEAD: f32 = 80.0;
#[cfg(not(target_os = "macos"))]
pub const TITLE_BAR_LEAD: f32 = 12.0;
