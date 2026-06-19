//! Color palette and layout constants.
//!
//! Colors track the "GitHub Dark Dimmed" variant of the GitHub Zed theme —
//! source of truth: https://github.com/PyaeSoneAungRgn/github-zed-theme
//! (themes/github_theme.json). Elevation ladder: chrome/panels darkest, the
//! editor canvas above it, modals/popovers elevated lightest.

pub const CANVAS: u32 = 0x212830; // editor.background (file list, window root)
pub const INSET: u32 = 0x151b23; // surface.background (panels, inputs, chrome)
pub const ELEVATED: u32 = 0x2a313c; // elevated_surface.background (modals, popovers)
pub const BORDER_MUTED: u32 = 0x3d444d; // border
pub const FG: u32 = 0xd1d7e0; // text
pub const FG_MUTED: u32 = 0x9198a1; // text.placeholder (secondary)
pub const FG_SUBTLE: u32 = 0x656c76; // tertiary
pub const ACCENT: u32 = 0x478be6; // text.accent
pub const ACCENT_HOVER: u32 = 0x5a9bf0;
/// Translucent accent fill for selected chips/segments.
pub const ACCENT_SOFT: u32 = 0x478be626;
pub const SUCCESS: u32 = 0x57ab5a; // success
pub const DANGER: u32 = 0xe5534b; // error
// Translucent element overlays (element.* base), neutral over any pane bg.
pub const OVERLAY: u32 = 0x656c7626;
pub const SELECT: u32 = 0x656c7659;
pub const SELECT_MUTED: u32 = 0x656c7633;

pub const SEPARATOR: u32 = 0x2d343d;
/// File-list column default widths and drag bounds. `COL_SIZE`/`COL_MIN` fit
/// the widest `human_size` value (e.g. "1023.9 MB") so it never wraps or clips.
pub const COL_DATE: f32 = 136.0;
pub const COL_SIZE: f32 = 88.0;
pub const COL_MIN: f32 = 72.0;
pub const COL_MAX: f32 = 340.0;
/// Row/header outer horizontal padding (`px_3`), in px — referenced by the
/// column-resize math, so keep it in sync with the layout. Columns are flush
/// (no inter-column gap); each insets its content with its own `px_2`, so the
/// resize divider sits exactly on the column boundary.
pub const TABLE_PAD: f32 = 12.0;
/// Fixed file-list row height, shared by entry rows and the inline editor so
/// renaming/new-folder never shifts the layout. Scaled by the UI zoom.
pub const ROW_H: f32 = 28.0;

/// The rem size the `rem()`-based sizes were authored at; the live rem size is
/// the user's `ui_font_size`, so all rem sizing scales from it.
pub const BASE_REM: f32 = 16.0;
/// Default + clamp for the user's UI font size (px), à la Zed's `ui_font_size`.
pub const UI_FONT_DEFAULT: f32 = 16.0;
pub const UI_FONT_MIN: f32 = 10.0;
pub const UI_FONT_MAX: f32 = 28.0;

pub const SIDEBAR_W: f32 = 204.0;
pub const SIDEBAR_MIN: f32 = 160.0;
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
