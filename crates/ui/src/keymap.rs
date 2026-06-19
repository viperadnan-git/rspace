//! Single source of truth for key bindings. The app binds from [`commands`];
//! the command palette and the keybindings reference both render from it, so a
//! shortcut is defined in exactly one place.

use gpui::{Action, App, KeyBinding};

use crate::confirm::ConfirmAccept;
use crate::explorer::{CloseSearch, SearchSubmit};
use crate::keybindings::DismissKeybindings;
use crate::mount_options::{MountCancel, MountSave};
use crate::number_field::NumberCommit;
use crate::prompt::{PromptCancel, PromptSubmit};
use crate::remotes::{ConfigConfirm, ConfigNext, ConfigPrev, FocusNext, FocusPrev};
use crate::status_screen::SetupSubmit;
use crate::*;

/// Reference groups, in display order.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Category {
    General,
    Navigation,
    Selection,
    File,
    View,
    Window,
    Dialogs,
}

impl Category {
    pub(crate) const ORDER: [Category; 7] = [
        Category::General,
        Category::Navigation,
        Category::Selection,
        Category::File,
        Category::View,
        Category::Window,
        Category::Dialogs,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Navigation => "Navigation",
            Category::Selection => "Selection",
            Category::File => "File",
            Category::View => "View",
            Category::Window => "Window",
            Category::Dialogs => "Dialogs",
        }
    }
}

/// One command: its bindings (zero or more keystrokes), where it applies, and how
/// it surfaces in the palette / reference.
pub(crate) struct Command {
    pub category: Category,
    pub label: &'static str,
    /// Whether the command palette offers it (independent of having a keystroke).
    pub in_palette: bool,
    /// One [`KeyBinding`] per keystroke (empty for palette-only commands).
    pub bindings: Vec<KeyBinding>,
    /// A boxed action for palette dispatch.
    pub action: Box<dyn Action>,
}

fn cmd<A: Action + Clone>(
    category: Category,
    label: &'static str,
    keystrokes: &[&'static str],
    action: A,
    context: Option<&'static str>,
    in_palette: bool,
) -> Command {
    let bindings = keystrokes.iter().map(|k| KeyBinding::new(k, action.clone(), context)).collect();
    Command { category, label, in_palette, bindings, action: Box::new(action) }
}

/// Bind every keystroke into the keymap. The single binding step for the app.
pub(crate) fn bind(cx: &mut App) {
    cx.bind_keys(commands().into_iter().flat_map(|c| c.bindings));
}

/// Every command, in reference-display order. Faithful port of the former
/// `bootstrap::bind_keys` table, enriched with labels/categories.
pub(crate) fn commands() -> Vec<Command> {
    // Navigation/mutation are inert while a dialog owns the keyboard.
    let ws = Some("Workspace && !modal && !TextInput");
    let ws_modal = Some("Workspace && !modal");
    let mut v = vec![
        // General
        cmd(Category::General, "Command palette", &["secondary-k"], TogglePalette, Some("Workspace"), false),
        cmd(Category::General, "Keyboard shortcuts", &["secondary-/"], ShowKeybindings, ws_modal, true),
        cmd(Category::General, "Close / dismiss", &["escape"], CloseSettings, Some("Workspace"), false),
        cmd(Category::General, "Add remote", &[], AddRemote, None, true),
        cmd(Category::General, "Open settings", &[], OpenSettings, None, true),
        cmd(Category::General, "Restart daemon", &[], RestartDaemon, None, true),
        cmd(Category::General, "Quit", &["secondary-q"], Quit, None, true),
        // Navigation
        cmd(Category::Navigation, "Open", &["enter"], Open, ws, false),
        cmd(Category::Navigation, "Toggle pane", &["tab"], TogglePane, ws, true),
        cmd(Category::Navigation, "Go up", &["backspace"], GoUp, ws, true),
        cmd(Category::Navigation, "Go back", &["secondary-["], GoBack, ws, true),
        cmd(Category::Navigation, "Go forward", &["secondary-]"], GoForward, ws, true),
        cmd(Category::Navigation, "Focus sidebar", &["left"], FocusSidebar, ws, true),
        cmd(Category::Navigation, "Focus explorer", &["right"], FocusExplorer, ws, true),
        // Selection
        cmd(Category::Selection, "Select next", &["down", "j", "shift-down", "shift-j"], SelectNext, ws, false),
        cmd(Category::Selection, "Select previous", &["up", "k", "shift-up", "shift-k"], SelectPrev, ws, false),
        cmd(Category::Selection, "Select all", &["secondary-a"], SelectAll, ws, true),
        // File
        cmd(Category::File, "Copy", &["secondary-c"], CopyEntry, ws, false),
        cmd(Category::File, "Cut", &["secondary-x"], CutEntry, ws, false),
        cmd(Category::File, "Paste", &["secondary-v"], PasteEntry, ws, false),
        cmd(Category::File, "Delete", &["secondary-backspace"], DeleteEntry, ws, false),
        cmd(Category::File, "New folder", &["secondary-shift-n"], NewFolder, ws, true),
        cmd(Category::File, "New file", &["secondary-u"], NewFile, ws, true),
        cmd(Category::File, "Rename", &["f2"], Rename, ws, false),
        // View
        cmd(Category::View, "Reload", &["secondary-r"], Reload, ws, true),
        cmd(Category::View, "Toggle preview", &["space"], TogglePreview, ws, true),
        cmd(Category::View, "Toggle tasks panel", &["secondary-t"], ToggleTasks, ws_modal, true),
        cmd(Category::View, "Toggle search", &["secondary-f"], ToggleSearch, ws_modal, false),
        cmd(Category::View, "Zoom in", &["secondary-=", "secondary-+"], ZoomIn, ws_modal, false),
        cmd(Category::View, "Zoom out", &["secondary--"], ZoomOut, ws_modal, false),
        cmd(Category::View, "Reset zoom", &["secondary-0"], ZoomReset, ws_modal, false),
        // Window
        cmd(Category::Window, "Toggle fullscreen", &["ctrl-cmd-f", "f11"], ToggleFullscreen, None, true),
        cmd(Category::Window, "Zoom window", &[], Zoom, None, true),
        // Dialogs — contextual, not user-curated commands.
        cmd(Category::Dialogs, "Next item", &["down", "ctrl-n"], ConfigNext, Some("RemoteConfig"), false),
        cmd(Category::Dialogs, "Previous item", &["up", "ctrl-p"], ConfigPrev, Some("RemoteConfig"), false),
        cmd(Category::Dialogs, "Confirm step", &["enter"], ConfigConfirm, Some("RemoteConfig"), false),
        cmd(Category::Dialogs, "Next field", &["tab"], FocusNext, Some("RemoteConfig"), false),
        cmd(Category::Dialogs, "Previous field", &["shift-tab"], FocusPrev, Some("RemoteConfig"), false),
        cmd(Category::Dialogs, "Accept", &["enter"], ConfirmAccept, Some("Confirm"), false),
        cmd(Category::Dialogs, "Submit prompt", &["enter"], PromptSubmit, Some("Prompt"), false),
        cmd(Category::Dialogs, "Cancel prompt", &["escape"], PromptCancel, Some("Prompt"), false),
        cmd(Category::Dialogs, "Save mount options", &["enter"], MountSave, Some("MountOptions"), false),
        cmd(Category::Dialogs, "Cancel mount options", &["escape"], MountCancel, Some("MountOptions"), false),
        cmd(Category::Dialogs, "Submit setup", &["enter"], SetupSubmit, Some("Setup"), false),
        cmd(Category::Dialogs, "Commit value", &["enter"], NumberCommit, Some("NumberField"), false),
        cmd(Category::Dialogs, "Run search", &["enter"], SearchSubmit, Some("ExplorerSearch"), false),
        cmd(Category::Dialogs, "Close search", &["escape"], CloseSearch, Some("ExplorerSearch"), false),
        // Dismiss the keybindings reference itself.
        cmd(Category::Dialogs, "Close shortcuts", &["escape"], DismissKeybindings, Some("Keybindings"), false),
    ];
    // Minimize is a macOS app convention (cmd-m); elsewhere the window manager owns
    // it, so it stays a palette-only command there.
    v.push(if cfg!(target_os = "macos") {
        cmd(Category::Window, "Minimize", &["cmd-m"], Minimize, None, true)
    } else {
        cmd(Category::Window, "Minimize", &[], Minimize, None, true)
    });
    v
}
