//! The Sync dock panel: shows the two split endpoints (source left, dest right),
//! runs Compare, and summarizes the result. The diff itself is shown as markers in
//! the two file lists; this panel drives the comparison and (later) the sync.

use gpui::WeakEntity;

use super::*;

pub(crate) struct SyncPane {
    workspace: WeakEntity<Workspace>,
}

impl SyncPane {
    pub(crate) fn new(workspace: WeakEntity<Workspace>, _cx: &mut Context<Self>) -> Self {
        Self { workspace }
    }
}

/// A count with its diff-state dot — the same colors as the file-row markers, so
/// the summary reads as a legend for what's highlighted in the lists.
fn count_chip(color: u32, n: usize, label: &str) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .items_center()
        .text_color(rgb(FG_MUTED))
        .child(div().flex_shrink_0().size(px(7.0)).rounded_full().bg(rgb(color)))
        .child(format!("{n} {label}"))
}

impl Render for SyncPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(ws) = self.workspace.upgrade() else {
            return v_flex().into_any_element();
        };
        let (endpoints, counts, comparing) = {
            let w = ws.read(cx);
            (w.sync_endpoints(), w.compare_counts(), w.comparing())
        };

        let Some((src, dst)) = endpoints else {
            return div()
                .p_3()
                .text_sm()
                .text_color(rgb(FG_SUBTLE))
                .child("Split the view to compare two folders.")
                .into_any_element();
        };

        // One endpoint: a quiet label over the remote:path, truncating under pressure.
        let endpoint = |label: &'static str, value: String| {
            v_flex()
                .min_w(px(0.0))
                .gap_0p5()
                .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(label))
                .child(div().min_w(px(0.0)).truncate().text_color(rgb(FG)).child(value))
        };

        v_flex()
            .w_full()
            .p_3()
            .gap_3()
            .child(endpoint("Source", src))
            .child(endpoint("Destination", dst))
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("sync-compare", "Compare", ButtonStyle::Primary)
                            .size(ControlSize::Small)
                            .build(
                                |this: &mut Self, cx| {
                                    this.workspace.update(cx, |ws, cx| ws.run_compare(cx)).ok();
                                },
                                cx,
                            ),
                    )
                    .when(comparing, |el| {
                        el.child(div().text_xs().text_color(rgb(FG_MUTED)).child("Comparing\u{2026}"))
                    }),
            )
            .map(|el| match counts {
                Some((differ, src_only, dst_only, matched)) => el.child(
                    v_flex()
                        .gap_1p5()
                        .pt_1()
                        .border_t_1()
                        .border_color(rgb(BORDER_MUTED))
                        .text_sm()
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_x_4()
                                .gap_y_1()
                                .child(count_chip(ACCENT, differ, "changed"))
                                .child(count_chip(SUCCESS, src_only, "only left"))
                                .child(count_chip(DANGER, dst_only, "only right")),
                        )
                        .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(format!("{matched} identical"))),
                ),
                None => el.child(
                    div()
                        .text_xs()
                        .text_color(rgb(FG_SUBTLE))
                        .child("Compare to see what differs between the two folders."),
                ),
            })
            .into_any_element()
    }
}
