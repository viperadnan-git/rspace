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

impl Render for StatusScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        focus_once(&mut self.focused, &self.focus_handle, window, cx);
        let missing = matches!(self.rclone, RcloneStatus::Missing { .. });
        let (heading, sub): (SharedString, SharedString) = match &self.rclone {
            RcloneStatus::Error { message } => ("rclone won't start".into(), message.clone().into()),
            _ => (
                "Set up rclone".into(),
                "rspace uses the rclone binary to reach your cloud storage.".into(),
            ),
        };

        let header = v_flex()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .text_size(rem(21.0))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(FG))
                    .child(heading),
            )
            .child(div().max_w(rem(300.0)).text_sm().text_center().text_color(rgb(FG_MUTED)).child(sub));

        let install_url = rspace_rclone_rc::INSTALL_URL.to_string();
        let actions = v_flex()
            .w_full()
            .max_w(rem(340.0))
            .items_center()
            .gap_4()
            .when(missing, |el| {
                el.child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(Button::new("setup-install", "Install rclone", ButtonStyle::Primary).build(
                            move |_, cx| cx.open_url(&install_url),
                            cx,
                        ))
                        .child(Button::new("setup-recheck", "Check again", ButtonStyle::Soft).build(
                            |this, cx| this.check_again(cx),
                            cx,
                        )),
                )
            })
            .when(missing, |el| el.child(divider()))
            // The manual-path form stays hidden behind a quiet link to keep the
            // first impression minimal.
            .when(!self.show_path, |el| {
                el.child(text_link("setup-reveal-path", "Enter rclone path manually", None, |this, window, cx| {
                    this.show_path = true;
                    this.path_input.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                }, cx))
            })
            .when(self.show_path, |el| {
                el.child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(div().flex_1().min_w(px(0.0)).child(self.path_input.clone()))
                                .child(Button::new("setup-browse", "Browse\u{2026}", ButtonStyle::Soft).build(|this, cx| {
                                    this.browse(cx)
                                }, cx)),
                        )
                        .when_some(self.error.clone(), |el, e| {
                            el.child(div().text_xs().text_color(rgb(DANGER)).child(e))
                        })
                        .child(
                            h_flex().w_full().justify_center().child(
                                Button::new("setup-save", "Use this path", ButtonStyle::Primary)
                                    .build(|this, cx| this.do_submit(cx), cx),
                            ),
                        ),
                )
            });

        let footer = h_flex().w_full().justify_center().pb_8().child(
            text_link("gh-link", "viperadnan-git/rspace", Some("icons/github.svg"), |_, _, cx| {
                cx.open_url(REPO_URL)
            }, cx)
            .tooltip(tooltip_text(REPO_URL)),
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
                    .flex_1()
                    .min_h(px(0.0))
                    .items_center()
                    .justify_center()
                    .gap_7()
                    .p_8()
                    .child(brand_mark())
                    .child(header)
                    .child(actions),
            )
            .child(footer)
    }
}
