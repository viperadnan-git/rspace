//! Per-remote mount options modal: VFS cache mode, read-only, and cache limits,
//! mapped to rclone mount flags. Presentational — emits the chosen config; the
//! owning [`Workspace`] persists it and (re)mounts.

use gpui::{ClickEvent, Entity, EventEmitter, FocusHandle, Focusable};

use super::*;
use crate::text_input::TextInput;
use rspace_rclone_rc::{CacheMode, MountConfig};

pub(crate) enum MountOptionsEvent {
    Save(MountConfig),
    Dismiss,
}

pub(crate) struct MountOptionsModal {
    focus_handle: FocusHandle,
    remote: SharedString,
    cache_mode: CacheMode,
    read_only: bool,
    cache_size: Entity<TextInput>,
    cache_age: Entity<TextInput>,
    ro_focus: FocusHandle,
    focused: bool,
}

impl EventEmitter<MountOptionsEvent> for MountOptionsModal {}

impl Focusable for MountOptionsModal {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MountOptionsModal {
    pub(crate) fn new(
        remote: impl Into<SharedString>,
        config: MountConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let cache_size = cx.new(|cx| TextInput::new(cx, "e.g. 10G \u{2014} empty for unlimited"));
        if !config.cache_max_size.is_empty() {
            cache_size.update(cx, |i, cx| i.set_text(config.cache_max_size.clone(), cx));
        }
        let cache_age = cx.new(|cx| TextInput::new(cx, "e.g. 1h \u{2014} empty for default"));
        if !config.cache_max_age.is_empty() {
            cache_age.update(cx, |i, cx| i.set_text(config.cache_max_age.clone(), cx));
        }
        Self {
            focus_handle: cx.focus_handle(),
            remote: remote.into(),
            cache_mode: config.cache_mode,
            read_only: config.read_only,
            cache_size,
            cache_age,
            ro_focus: cx.focus_handle(),
            focused: false,
        }
    }

    fn config(&self, cx: &App) -> MountConfig {
        MountConfig {
            cache_mode: self.cache_mode,
            read_only: self.read_only,
            cache_max_size: self.cache_size.read(cx).text().trim().to_string(),
            cache_max_age: self.cache_age.read(cx).text().trim().to_string(),
        }
    }

    fn emit_save(&self, cx: &mut Context<Self>) {
        cx.emit(MountOptionsEvent::Save(self.config(cx)));
    }

    fn save(&mut self, _: &MountSave, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_save(cx);
    }

    fn cancel(&mut self, _: &MountCancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(MountOptionsEvent::Dismiss);
    }

    fn mode_chip(&self, mode: CacheMode, label: &'static str, cx: &mut Context<Self>) -> Stateful<Div> {
        chip(SharedString::from(format!("cm-{}", mode.as_arg())), label, self.cache_mode == mode)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.cache_mode = mode;
                cx.notify();
            }))
    }
}

impl Workspace {
    /// Open the mount-options modal for `remote`, seeded with its saved config.
    pub(crate) fn begin_mount_options(&mut self, remote: String, cx: &mut Context<Self>) {
        let config = self.mount_config_for(&remote);
        let modal = cx.new(|cx| MountOptionsModal::new(remote.clone(), config, cx));
        self.mount_options_sub = Some(cx.subscribe(&modal, move |this, _, event, cx| {
            match event {
                MountOptionsEvent::Save(config) => {
                    this.apply_mount_config(remote.clone(), config.clone(), cx);
                    this.mount_options = None;
                }
                MountOptionsEvent::Dismiss => this.mount_options = None,
            }
            cx.notify();
        }));
        self.mount_options = Some(modal);
        cx.notify();
    }
}

impl Render for MountOptionsModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        modal_card("mount-options-card", &self.focus_handle, cx)
            .key_context("modal MountOptions")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::cancel))
            .w(px(420.0))
            .gap_4()
            .child(
                div()
                    .text_lg()
                    .text_color(rgb(FG))
                    .child(format!("Mount options \u{2014} {}", self.remote)),
            )
            .child(form_field(
                "Cache mode",
                "Streaming suits media (reads stream); Full suits editing (caches reads).",
                false,
                h_flex()
                    .gap_2()
                    .child(self.mode_chip(CacheMode::Off, "Off", cx))
                    .child(self.mode_chip(CacheMode::Minimal, "Minimal", cx))
                    .child(self.mode_chip(CacheMode::Writes, "Streaming", cx))
                    .child(self.mode_chip(CacheMode::Full, "Full", cx)),
            ))
            .child(form_field(
                "Read-only",
                "Reject writes through the mount.",
                false,
                switch("mo-readonly", self.read_only, Some(&self.ro_focus), |this, cx| {
                    this.read_only = !this.read_only;
                    cx.notify();
                }, cx),
            ))
            .child(form_field("Cache size limit", "", false, self.cache_size.clone()))
            .child(form_field("Cache max age", "", false, self.cache_age.clone()))
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(Button::new("mo-cancel", "Cancel", ButtonStyle::Secondary).build(
                        |_, cx| cx.emit(MountOptionsEvent::Dismiss),
                        cx,
                    ))
                    .child(Button::new("mo-save", "Save", ButtonStyle::Primary).build(
                        |this, cx| this.emit_save(cx),
                        cx,
                    )),
            )
    }
}
