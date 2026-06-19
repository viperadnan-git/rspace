//! The welcome / landing screen shown when no remote is open.

use super::*;

impl Workspace {
    pub(super) fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette_key: &str = if cfg!(target_os = "macos") { "\u{2318}K" } else { "Ctrl K" };
        let has_remotes = !self.remotes.is_empty();
        let prompt = if has_remotes {
            "Select a remote from the sidebar to start."
        } else {
            "Add a remote to start browsing your cloud."
        };

        let brand = brand_mark();

        // Recently-opened remotes still present in the config, newest first.
        // Filter the cached recents against the live config, then cap — so
        // since-deleted remotes don't shrink the list below what's available.
        let recent: Vec<RemoteInfo> = self
            .recent_remotes
            .iter()
            .filter_map(|n| self.remotes.iter().find(|r| &r.name == n).cloned())
            .take(RECENT_REMOTES_SHOWN)
            .collect();

        v_flex()
            .flex_1()
            .min_h(px(0.0))
            .items_center()
            .justify_center()
            .gap_4()
            .p_8()
            .child(brand)
            .when(recent.is_empty(), |el| {
                el.child(div().text_sm().text_color(rgb(FG_MUTED)).child(prompt))
            })
            .when(!recent.is_empty(), |el| {
                el.child(
                    h_flex()
                        .flex_wrap()
                        .justify_center()
                        .gap_1p5()
                        .max_w(px(360.0))
                        .children(recent.into_iter().enumerate().map(|(ix, r)| self.recent_remote_row(ix, r, cx))),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(key_binding(palette_key))
                    .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child("Run a command")),
            )
            .when(!has_remotes, |el| {
                el.child(Button::new("welcome-add", "Add remote", ButtonStyle::Secondary).build(
                    |this, cx| this.begin_add_remote(cx),
                    cx,
                ))
            })
    }

    fn recent_remote_row(&self, ix: usize, remote: RemoteInfo, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = remote.name.clone();
        h_flex()
            .id(("recent-remote", ix))
            .flex_shrink_0()
            .gap_1p5()
            .px_2()
            .py_0p5()
            .rounded_md()
            .border_1()
            .border_color(rgb(BORDER_MUTED))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(OVERLAY)))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.navigate(name.clone(), String::new(), None, cx)
            }))
            .child(svg().path(remote_icon(&remote.kind)).size(px(12.0)).flex_shrink_0().text_color(rgb(FG_MUTED)))
            .child(div().text_xs().text_color(rgb(FG)).child(remote.name.clone()))
    }

}
