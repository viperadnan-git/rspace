//! The welcome / landing screen shown when no remote is open.

use super::*;

impl Workspace {
    pub(crate) fn render_welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
        let frequent: Vec<RemoteInfo> = self
            .frequent_remotes
            .iter()
            .filter_map(|n| self.remotes.iter().find(|r| &r.name == n).cloned())
            .take(FREQUENT_REMOTES_SHOWN)
            .collect();

        v_flex()
            .flex_1()
            .min_h(px(0.0))
            .items_center()
            .justify_center()
            .gap_4()
            .p_8()
            .child(brand)
            .when(frequent.is_empty(), |el| {
                el.child(div().text_sm().text_color(rgb(FG_MUTED)).child(prompt))
            })
            .when(!frequent.is_empty(), |el| {
                el.child(
                    h_flex()
                        .flex_wrap()
                        .justify_center()
                        .gap_1p5()
                        .max_w(rem(360.0))
                        .children(frequent.into_iter().enumerate().map(|(ix, r)| self.frequent_remote_row(ix, r, cx))),
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
                el.child(Button::new("welcome-add", "Add remote", ButtonStyle::Ghost).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.begin_add_remote(cx)),
                ))
            })
    }

    fn frequent_remote_row(&self, ix: usize, remote: RemoteInfo, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let name = remote.name.clone();
        h_flex()
            .id(("frequent-remote", ix))
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
            .child(svg().path(remote_icon(&remote.kind)).size(rem(12.0)).flex_shrink_0().text_color(rgb(FG_MUTED)))
            .child(div().text_xs().text_color(rgb(FG)).child(remote.name.clone()))
    }

}
