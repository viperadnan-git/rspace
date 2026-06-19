//! Rendering for the add/edit-remote modal (see [`super`]).

use gpui::Entity;
use serde_json::Value;

use super::*;
use crate::text_input::TextInput;

impl Render for RemoteConfigModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.focus_primary(window, cx);
        let title = match (self.mode, &self.phase) {
            (ConfigMode::Add, Phase::PickType) => "Add remote".to_string(),
            (ConfigMode::Add, _) => format!("Add {} remote", self.kind),
            (ConfigMode::Edit, _) => format!("Edit {}", self.name.read(cx).text()),
        };
        let body = match self.phase {
            Phase::Busy => div().flex_1().child(loading_view()).into_any_element(),
            Phase::PickType => self.config_picker(cx).into_any_element(),
            Phase::Form => self.config_form(cx).into_any_element(),
            Phase::Question => self.config_question(cx).into_any_element(),
        };
        modal_card("remote-config-card", &self.focus_handle, cx)
            // "modal" suppresses the workspace's `!modal` shortcuts; "RemoteConfig"
            // scopes the dialog's own key bindings.
            .key_context("modal RemoteConfig")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::config_next))
            .on_action(cx.listener(Self::config_prev))
            .on_action(cx.listener(Self::config_confirm))
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_prev))
            .w(rem(520.0))
            .h(rem(520.0))
            .gap_3()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(div().text_lg().text_color(rgb(FG)).child(title))
                    .child(
                        focus_ring(icon_button("cfg-close", "icons/x.svg"))
                            .track_focus(&self.close_focus)
                            .tab_index(0)
                            .tooltip(tooltip_text("Close"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.dismiss(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(div().flex_shrink_0().text_xs().text_color(rgb(DANGER)).child(e))
            })
            .child(body)
    }
}

impl RemoteConfigModal {
    fn config_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let backends = self.filtered_backends(cx);
        let sel = self.picker_sel;
        let list = uniform_list(
            "cfg-providers",
            backends.len(),
            cx.processor(move |_this, range: Range<usize>, _window, cx| {
                range
                    .filter_map(|i| backends.get(i).map(|b| (i, b.clone())))
                    .map(|(i, (name, desc))| {
                        let kind = name.clone();
                        nav_item(i, i == sel, true)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.pick_backend(kind.clone(), cx)
                            }))
                            .child(
                                svg().path(remote_icon(&name)).size(rem(16.0)).flex_shrink_0().text_color(rgb(FG_MUTED)),
                            )
                            .child(div().flex_shrink_0().text_sm().text_color(rgb(FG)).child(name))
                            .child(
                                div().min_w(px(0.0)).truncate().text_xs().text_color(rgb(FG_SUBTLE)).child(desc),
                            )
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.picker_scroll)
        .flex_1();
        v_flex().flex_1().min_h(px(0.0)).gap_2().child(self.search.clone()).child(list)
    }

    fn config_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_advanced = self.options.iter().any(|o| o.advanced);
        v_flex()
            .flex_1()
            .min_h(px(0.0))
            .gap_3()
            .child(
                v_flex()
                    .id("cfg-fields")
                    .tab_group()
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_scroll()
                    .gap_3()
                    .child(form_field("Name", "A name for this remote", true, self.name.clone().into_any_element()))
                    .children(self.options.iter().filter(|o| !o.advanced).map(|o| self.config_option(o, cx)))
                    .when(has_advanced && self.show_advanced, |el| {
                        el.children(self.options.iter().filter(|o| o.advanced).map(|o| self.config_option(o, cx)))
                    }),
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        // "Show advanced" sits on the left of the action row.
                        div().when(has_advanced, |el| {
                            let label = if self.show_advanced { "Hide advanced" } else { "Show advanced" };
                            el.child(
                                focus_ring(h_flex().id("cfg-adv"))
                                    .track_focus(&self.advanced_focus)
                                    .tab_index(0)
                                    .px_1()
                                    .py(px(2.0))
                                    .rounded_md()
                                    .text_xs()
                                    .text_color(rgb(ACCENT))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgba(OVERLAY)))
                                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_advanced(cx)))
                                    .child(label),
                            )
                        }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(self.config_button(&self.cancel_focus, "cfg-cancel", "Cancel", false, |this, cx| this.dismiss(cx), cx))
                            .child(self.config_button(
                                &self.primary_focus,
                                "cfg-save",
                                if self.mode == ConfigMode::Add { "Create" } else { "Save" },
                                true,
                                |this, cx| this.submit_config(cx),
                                cx,
                            )),
                    ),
            )
    }

    fn config_question(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.question.as_ref().unwrap();
        v_flex()
            .flex_1()
            .min_h(px(0.0))
            .gap_3()
            .child(
                v_flex().id("cfg-question").flex_1().min_h(px(0.0)).overflow_scroll().child(form_field(
                    &q.name,
                    q.help.lines().next().unwrap_or_default(),
                    q.required,
                    self.answer_widget(q, cx),
                )),
            )
            .child(
                h_flex().justify_end().gap_2().child(self.config_button(
                    &self.primary_focus,
                    "cfg-continue",
                    "Continue",
                    true,
                    |this, cx| this.answer_question(cx),
                    cx,
                )),
            )
    }

    fn config_option(&self, opt: &RemoteOption, cx: &mut Context<Self>) -> AnyElement {
        let widget: AnyElement = if opt.kind == "bool" {
            let on = self.bools.get(&opt.name).and_then(Value::as_str) == Some("true");
            let name = opt.name.clone();
            switch(
                SharedString::from(format!("bool-{}", opt.name)),
                on,
                self.bool_focus.get(&opt.name),
                move |this, cx| this.set_bool(name.clone(), !on, cx),
                cx,
            )
            .into_any_element()
        } else if let Some(input) = self.fields.get(&opt.name) {
            if opt.examples.is_empty() {
                input.clone().into_any_element()
            } else {
                self.chips_with_input(opt, input.clone(), cx).into_any_element()
            }
        } else {
            div().into_any_element()
        };
        form_field(&opt.name, opt.help.lines().next().unwrap_or_default(), opt.required, widget)
            .into_any_element()
    }

    fn answer_widget(&self, q: &RemoteOption, cx: &mut Context<Self>) -> AnyElement {
        if q.kind == "bool" {
            let on = self.answer.read(cx).text() == "true";
            let answer = self.answer.clone();
            return switch("answer-bool-sw", on, None, move |_: &mut Self, cx| {
                answer.update(cx, |i, cx| i.set_text(if on { "false" } else { "true" }, cx));
            }, cx)
            .into_any_element();
        }
        if q.examples.is_empty() {
            self.answer.clone().into_any_element()
        } else {
            self.chips_with_input(q, self.answer.clone(), cx).into_any_element()
        }
    }

    /// Example chips that fill the field's input, plus the input itself.
    fn chips_with_input(
        &self,
        opt: &RemoteOption,
        input: Entity<TextInput>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let current = input.read(cx).text();
        let chips: Vec<_> = opt
            .examples
            .iter()
            .map(|ex| {
                let (value, target) = (ex.value.clone(), input.clone());
                let label = if ex.value.is_empty() { "(empty)" } else { &ex.value };
                chip(SharedString::from(format!("ex-{}-{}", opt.name, ex.value)), label.to_string(), ex.value == current)
                    .when(!ex.help.is_empty(), |el| el.tooltip(tooltip_text(ex.help.clone())))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        target.update(cx, |i, cx| i.set_text(value.clone(), cx));
                        cx.notify();
                    }))
            })
            .collect();
        v_flex().gap_1().child(h_flex().flex_wrap().gap_1().children(chips)).child(input)
    }

    fn config_button(
        &self,
        focus: &gpui::FocusHandle,
        id: &'static str,
        label: &'static str,
        primary: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let style = if primary { ButtonStyle::Primary } else { ButtonStyle::Secondary };
        focus_ring(Button::new(id, label, style).build(action, cx)).track_focus(focus).tab_index(0)
    }
}
