//! GitHub Dark Dimmed palette (Zed theme) and layout constants.

pub const CANVAS: u32 = 0x212830;
pub const INSET: u32 = 0x151b23;
pub const ELEVATED: u32 = 0x2a313c;
pub const BORDER_MUTED: u32 = 0x3d444d;
pub const FG: u32 = 0xd1d7e0;
pub const FG_MUTED: u32 = 0x9198a1;
pub const FG_SUBTLE: u32 = 0x656c76;
pub const ACCENT: u32 = 0x478be6;
pub const ACCENT_HOVER: u32 = 0x5a9bf0;
/// Translucent accent fill for selected chips/segments.
pub const ACCENT_SOFT: u32 = 0x478be626;
pub const SUCCESS: u32 = 0x57ab5a;
pub const DANGER: u32 = 0xe5534b;
// Translucent element overlays, neutral over either pane background.
pub const OVERLAY: u32 = 0x656c7626;
pub const SELECT: u32 = 0x656c7659;
pub const SELECT_MUTED: u32 = 0x656c7633;

/// Faint horizontal divider between file-list rows (Finder-style).
pub const SEPARATOR: u32 = 0x2d343d;
/// File-list column default widths and drag bounds.
pub const COL_DATE: f32 = 136.0;
pub const COL_SIZE: f32 = 72.0;
pub const COL_MIN: f32 = 52.0;
pub const COL_MAX: f32 = 340.0;
/// Row/header horizontal padding (`px_3`) and inter-column gap (`gap_2`), in px —
/// referenced by the column-resize math, so keep them in sync with the layout.
pub const TABLE_PAD: f32 = 12.0;
pub const COL_GAP: f32 = 8.0;

pub const SIDEBAR_W: f32 = 204.0;
pub const SIDEBAR_MIN: f32 = 160.0;
/// Default and bounds for the resizable file-preview pane.
pub const PREVIEW_W: f32 = 320.0;
pub const PREVIEW_MIN: f32 = 220.0;
pub const PREVIEW_MAX: f32 = 640.0;
pub const SIDEBAR_MAX: f32 = 480.0;
pub const RESIZE_HANDLE_W: f32 = 6.0;
pub const TITLE_BAR_H: f32 = 36.0;
pub const MAX_CRUMBS: usize = 4;

/// Left inset of the custom title bar to clear the window controls.
#[cfg(target_os = "macos")]
pub const TITLE_BAR_LEAD: f32 = 80.0;
#[cfg(not(target_os = "macos"))]
pub const TITLE_BAR_LEAD: f32 = 12.0;
