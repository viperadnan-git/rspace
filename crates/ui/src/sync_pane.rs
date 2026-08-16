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

/// A compact count: its diff-state dot (same colors as the file-row tints) and
/// the number, with the meaning as a tooltip.
fn count_dot(color: u32, n: usize, tip: &'static str) -> impl IntoElement {
    h_flex()
        .id(tip)
        .gap_1()
        .items_center()
        .text_color(rgb(FG_MUTED))
        .tooltip(tooltip_text(tip))
        .child(div().flex_shrink_0().size(px(7.0)).rounded_full().bg(rgb(color)))
        .child(format!("{n}"))
}

impl Render for SyncPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(ws) = self.workspace.upgrade() else {
            return v_flex().into_any_element();
        };
        let (endpoints, counts, comparing, mode, resync, has_compare) = {
            let w = ws.read(cx);
            (
                w.sync_endpoints(cx),
                w.compare_counts(),
                w.comparing(),
                w.sync_mode(),
                w.bisync_resync(),
                w.has_compare(),
            )
        };

        let Some((src, dst)) = endpoints else {
            return div()
                .p_3()
                .text_sm()
                .text_color(rgb(FG_SUBTLE))
                .child("Split the view to compare two folders.")
                .into_any_element();
        };

        // One endpoint on a single line: a fixed-width caption, then the path
        // (truncated, full path on hover).
        let endpoint = |caption: &'static str, value: String| {
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .text_sm()
                .child(div().flex_shrink_0().w(px(34.0)).text_xs().text_color(rgb(FG_SUBTLE)).child(caption))
                .child(
                    div()
                        .id(caption)
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(rgb(FG))
                        .tooltip(tooltip_text(value.clone()))
                        .child(value),
                )
        };

        v_flex()
            .w_full()
            .p_2()
            .gap_2()
            // Endpoints with a swap button on the right.
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        v_flex().flex_1().min_w(px(0.0)).gap_1().child(endpoint("From", src)).child(endpoint("To", dst)),
                    )
                    .child(
                        icon_button("sync-swap", "icons/swap.svg")
                            .tooltip(tooltip_text("Swap sides"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.workspace.update(cx, |ws, cx| ws.swap_panes(cx)).ok();
                            })),
                    ),
            )
            // Compare + clear, with the result tallied inline.
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("sync-compare", "Compare", ButtonStyle::Secondary)
                            .size(ControlSize::Small)
                            .loading(comparing)
                            .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _, cx| {
                                this.workspace.update(cx, |ws, cx| ws.run_compare(cx)).ok();
                            })),
                    )
                    .when(has_compare && !comparing, |el| {
                        el.child(
                            icon_button("sync-clear", "icons/x.svg")
                                .tooltip(tooltip_text("Clear result"))
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.workspace.update(cx, |ws, cx| ws.clear_compare(cx)).ok();
                                })),
                        )
                    })
                    .child(div().flex_1())
                    .when_some(counts, |el, (differ, src_only, dst_only, matched)| {
                        el.child(
                            h_flex()
                                .gap_2p5()
                                .text_sm()
                                .child(count_dot(SUCCESS, src_only, "Only on the left"))
                                .child(count_dot(ACCENT, differ, "Changed on both"))
                                .child(count_dot(DANGER, dst_only, "Only on the right"))
                                .child(div().text_color(rgb(FG_SUBTLE)).child(format!("{matched} same"))),
                        )
                    }),
            )
            // Mode + run.
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        h_flex()
                            .gap(px(2.0))
                            .p(px(2.0))
                            .rounded_md()
                            .bg(rgba(OVERLAY))
                            .child(mode_segment(SyncMode::Copy, "seg-copy", mode, cx))
                            .child(mode_segment(SyncMode::Mirror, "seg-mirror", mode, cx))
                            .child(mode_segment(SyncMode::Bisync, "seg-bisync", mode, cx)),
                    )
                    .child(div().flex_1())
                    .when(mode == SyncMode::Bisync, |el| {
                        el.child(
                            div().text_xs().child(
                                Checkbox::new("sync-resync", resync)
                                    .label("Resync")
                                    .tooltip("Establish a fresh baseline (first run)")
                                    .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _, cx| {
                                        this.workspace.update(cx, |ws, cx| ws.toggle_resync(cx)).ok();
                                    })),
                            ),
                        )
                    })
                    .child(
                        Button::new(
                            "sync-run",
                            // Label follows the mode: "Copy →" / "Sync →" / "Bisync ⇄".
                            match mode {
                                SyncMode::Bisync => format!("{} \u{21c4}", mode.label()),
                                _ => format!("{} \u{2192}", mode.label()),
                            },
                            ButtonStyle::Primary,
                        )
                        .size(ControlSize::Small)
                        .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _, cx| {
                            this.workspace.update(cx, |ws, cx| ws.start_sync(cx)).ok();
                        })),
                    ),
            )
            .child(div().text_xs().text_color(rgb(FG_SUBTLE)).child(mode_hint(mode)))
            .into_any_element()
    }
}

fn mode_hint(mode: SyncMode) -> &'static str {
    match mode {
        SyncMode::Copy => "Add and update on the right; never deletes.",
        SyncMode::Mirror => "Make the right match the left, deleting extras.",
        SyncMode::Bisync => "Reconcile both folders two-way.",
    }
}

/// One segment of the mode picker — a connected radio control, distinct from the
/// standalone [`Button`]: a shared track with the selected segment raised.
fn mode_segment(
    m: SyncMode,
    id: &'static str,
    current: SyncMode,
    cx: &mut Context<SyncPane>,
) -> impl IntoElement {
    let selected = m == current;
    div()
        .id(id)
        .px_2()
        .py(px(2.0))
        .rounded_sm()
        .cursor_pointer()
        .text_xs()
        .map(|d| {
            if selected {
                d.bg(rgb(ELEVATED)).text_color(rgb(FG))
            } else {
                d.text_color(rgb(FG_MUTED)).hover(|s| s.bg(rgba(OVERLAY)))
            }
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.workspace.update(cx, |ws, cx| ws.set_sync_mode(m, cx)).ok();
        }))
        .child(m.label())
}
