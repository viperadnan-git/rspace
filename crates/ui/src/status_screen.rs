//! Pre-flight screen shown when rclone is missing or fails to start.

use super::*;

actions!(setup, [SetupSubmit]);

const REPO_URL: &str = "https://github.com/viperadnan-git/rspace";

/// Re-exec the app so a freshly-saved rclone path takes effect from a clean
/// start (avoids transitioning the daemon/window in place).
pub(crate) fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let _ = cmd.exec(); // replaces this process on success
    }
    #[cfg(not(unix))]
    {
        let _ = cmd.spawn();
        std::process::exit(0);
    }
}

pub(crate) struct StatusScreen {
    rclone: RcloneStatus,
    store: SettingsStore,
    path_input: Entity<TextInput>,
    focus_handle: FocusHandle,
    error: Option<SharedString>,
    show_path: bool,
    /// Focus the screen once on open (not every frame, which would steal focus
    /// back from the path input on click).
    focused: bool,
}

impl Focusable for StatusScreen {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl StatusScreen {
    pub(crate) fn new(rclone: RcloneStatus, store: SettingsStore, cx: &mut Context<Self>) -> Self {
        let placeholder =
            if cfg!(windows) { "C:\\path\\to\\rclone.exe" } else { "/usr/local/bin/rclone" };
        let path_input = cx.new(|cx| TextInput::new(cx, placeholder));
        if let Some(p) = store.get().rclone_path.clone() {
            path_input.update(cx, |i, cx| i.set_text(p, cx));
        }
        Self {
            rclone,
            store,
            path_input,
            focus_handle: cx.focus_handle(),
            error: None,
            show_path: false,
            focused: false,
        }
    }

    fn submit(&mut self, _: &SetupSubmit, _: &mut Window, cx: &mut Context<Self>) {
        self.do_submit(cx);
    }

    fn do_submit(&mut self, cx: &mut Context<Self>) {
        let path = self.path_input.read(cx).text().trim().to_string();
        if path.is_empty() {
            self.error = Some("Enter the path to the rclone binary".into());
            cx.notify();
            return;
        }
        if rspace_rclone_rc::from_path(&path).is_none() {
            self.error = Some(format!("No working rclone at \u{201c}{path}\u{201d}").into());
            cx.notify();
            return;
        }
        self.store.update(|s| s.rclone_path = Some(path));
        relaunch();
    }

    fn check_again(&mut self, cx: &mut Context<Self>) {
        if rspace_rclone_rc::detect().is_ok() {
            relaunch();
        } else {
            self.error = Some("rclone still isn't detected — install it, or enter its path below.".into());
            cx.notify();
        }
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        pick_file_into(self.path_input.clone(), cx);
    }

}

/// A tinted glyph chip leading the card header — accent when rclone is merely
/// missing, danger when it's present but won't start.
fn state_badge(icon: &'static str, tint: u32, soft: u32) -> impl IntoElement {
    h_flex()
        .flex_shrink_0()
        .size(rem(34.0))
        .justify_center()
        .rounded_lg()
        .bg(rgba(soft))
        .child(svg().path(icon).size(rem(18.0)).text_color(rgb(tint)))
}

/// One numbered step in the install track: ordinal chip, title + caption, and
/// the step's own action. The text column flexes/wraps (Zed `Callout` idiom) so
/// long captions never push the action past the card edge.
fn step_row(n: &'static str, title: &'static str, caption: &'static str, action: impl IntoElement) -> impl IntoElement {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_2p5()
        .items_center()
        .child(
            h_flex()
                .flex_shrink_0()
                .size(rem(20.0))
                .justify_center()
                .rounded_full()
                .bg(rgba(ACCENT_SOFT))
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(ACCENT))
                .child(n),
        )
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(rgb(FG)).child(title))
                .child(div().text_xs().text_color(rgb(FG_MUTED)).child(caption)),
        )
        .child(div().flex_shrink_0().child(action))
}

impl StatusScreen {
    fn reveal_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_path = true;
        self.path_input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    /// The install → detect track, shown when no rclone binary was found.
    fn install_track(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let install_url = rspace_rclone_rc::INSTALL_URL.to_string();
        v_flex()
            .w_full()
            .gap_2()
            .child(step_row(
                "1",
                "Install rclone",
                "Free and open source. Takes about a minute.",
                Button::new("setup-install", "Install", ButtonStyle::Primary)
                    .icon("icons/external_link.svg")
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&install_url))),
            ))
            .child(step_row(
                "2",
                "Let rspace find it",
                "Scans your PATH and the usual install spots.",
                Button::new("setup-recheck", "Check again", ButtonStyle::Secondary)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.check_again(cx))),
            ))
    }

    /// The failure block, shown when rclone is present but the daemon won't start.
    fn error_block(&self, message: &str, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .p_3()
                    .rounded_md()
                    .bg(rgba(DANGER_SOFT))
                    .text_xs()
                    .text_color(rgb(FG))
                    .child(message.to_string()),
            )
            .child(
                Button::new("setup-retry", "Try again", ButtonStyle::Primary)
                    .icon("icons/refresh.svg")
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.check_again(cx))),
            )
    }

    /// The manual-path disclosure shared by both states: a quiet link that opens
    /// an inline path field with Browse.
    fn manual_path(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(divider())
            .when(!self.show_path, |el| {
                el.child(
                    text_link("setup-reveal-path", "Already installed? Set the rclone path", None, |this, window, cx| {
                        this.reveal_path(window, cx);
                    }, cx)
                    .text_xs(),
                )
            })
            .when(self.show_path, |el| {
                el.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(div().text_xs().text_color(rgb(FG_MUTED)).child("Path to the rclone binary"))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(div().flex_1().min_w(px(0.0)).child(self.path_input.clone()))
                                .child(Button::new("setup-browse", "Browse\u{2026}", ButtonStyle::Secondary).on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.browse(cx)),
                                ))
                                .child(Button::new("setup-save", "Use", ButtonStyle::Primary).on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| this.do_submit(cx)),
                                )),
                        )
                        .when_some(self.error.clone(), |el, e| {
                            el.child(div().text_xs().text_color(rgb(DANGER)).child(e))
                        }),
                )
            })
    }
}

impl Render for StatusScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);

        let (badge, heading, sub): (_, SharedString, SharedString) = match &self.rclone {
            RcloneStatus::Error { .. } => (
                state_badge("icons/alert.svg", DANGER, DANGER_SOFT),
                "rclone won't start".into(),
                "It's installed, but the background service failed to launch.".into(),
            ),
            _ => (
                state_badge("icons/rclone.svg", ACCENT, ACCENT_SOFT),
                "Connect rclone".into(),
                "Point rspace at rclone once — then your remotes show up here.".into(),
            ),
        };

        let card_header = h_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .items_center()
            .child(badge)
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(div().text_size(rem(17.0)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(rgb(FG)).child(heading))
                    .child(div().text_xs().text_color(rgb(FG_MUTED)).child(sub)),
            );

        let body = match &self.rclone {
            RcloneStatus::Error { message } => self.error_block(message, cx).into_any_element(),
            _ => self.install_track(cx).into_any_element(),
        };

        let card = v_flex()
            .w_full()
            .max_w(rem(460.0))
            .min_w_0()
            .gap_2p5()
            .p_3()
            .rounded_lg()
            .bg(rgb(ELEVATED))
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .shadow_lg()
            .overflow_x_hidden()
            .child(card_header)
            .child(body)
            .child(self.manual_path(cx));

        let footer = h_flex().w_full().justify_center().pb_8().child(
            text_link("gh-link", "viperadnan-git/rspace", Some("icons/github.svg"), |_, _, cx| {
                cx.open_url(REPO_URL)
            }, cx)
            .tooltip(tooltip_text(REPO_URL))
            .text_xs(),
        );

        v_flex()
            .size_full()
            .bg(rgb(INSET))
            .text_color(rgb(FG))
            .key_context("Setup")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::submit))
            .child(
                v_flex()
                    .id("setup-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .p_5()
                    .pt_2()
                    .overflow_y_scroll() // ponytail: justify_center clips top when content > viewport, but onboarding fits; scroll is the backstop
                    .child(brand_mark())
                    .child(card),
            )
            .child(footer)
    }
}
