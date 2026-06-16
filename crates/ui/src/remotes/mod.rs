//! Add / edit a remote as a self-contained modal entity. The form is generated
//! from `config/providers` and submitted via rclone's interactive
//! `config/create`/`config/update` state machine — no per-backend code, so any
//! current or future backend works.

use std::collections::HashMap;

use gpui::{ClickEvent, Entity, EventEmitter, Focusable};
use serde_json::{Map, Value};

use super::*;
use crate::text_input::TextInput;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ConfigMode {
    Add,
    Edit,
}

#[derive(Clone, PartialEq)]
enum Phase {
    PickType,
    Form,
    /// A question rclone raised mid-flow (OAuth / conditional branch).
    Question,
    Busy,
}

/// Signals to the owning [`Workspace`].
pub(crate) enum RemoteConfigEvent {
    /// Close without changes.
    Dismiss,
    /// A remote was created/updated; reload the list, then close.
    Saved,
}

pub(crate) struct RemoteConfigModal {
    focus_handle: gpui::FocusHandle,
    service: Service,
    /// Existing remote names, for the add-mode uniqueness check.
    remote_names: Vec<String>,
    mode: ConfigMode,
    phase: Phase,
    providers: Vec<Provider>,
    kind: String,
    options: Vec<RemoteOption>,
    /// Bool fields (text-ish fields live in `fields` as inputs).
    bools: Map<String, Value>,
    fields: HashMap<String, Entity<TextInput>>,
    search: Entity<TextInput>,
    name: Entity<TextInput>,
    answer: Entity<TextInput>,
    show_advanced: bool,
    question: Option<RemoteOption>,
    state: String,
    error: Option<String>,
    /// Focus the phase's primary input on the next render.
    autofocus: bool,
    /// Highlighted backend in the picker (keyboard navigation).
    picker_sel: usize,
    picker_scroll: UniformListScrollHandle,
    /// In-flight config step; dropped on close to abort the rclone request
    /// (and any pending OAuth callback server).
    pending: Option<gpui::Task<()>>,
    /// Focus handles for the non-input controls, so Tab reaches them too.
    primary_focus: gpui::FocusHandle,
    cancel_focus: gpui::FocusHandle,
    advanced_focus: gpui::FocusHandle,
    close_focus: gpui::FocusHandle,
    bool_focus: HashMap<String, gpui::FocusHandle>,
}

impl EventEmitter<RemoteConfigEvent> for RemoteConfigModal {}

impl Focusable for RemoteConfigModal {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl RemoteConfigModal {
    fn new(
        mode: ConfigMode,
        edit_name: Option<&str>,
        service: Service,
        remote_names: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| TextInput::new(cx, "my-remote"));
        if let Some(n) = edit_name {
            name.update(cx, |i, cx| i.set_text(n.to_string(), cx));
        }
        Self {
            focus_handle: cx.focus_handle(),
            service,
            remote_names,
            mode,
            phase: Phase::Busy,
            providers: Vec::new(),
            kind: String::new(),
            options: Vec::new(),
            bools: Map::new(),
            fields: HashMap::new(),
            search: cx.new(|cx| TextInput::new(cx, "Search backends…")),
            name,
            answer: cx.new(|cx| TextInput::new(cx, "")),
            show_advanced: false,
            question: None,
            state: String::new(),
            error: None,
            autofocus: true,
            picker_sel: 0,
            picker_scroll: UniformListScrollHandle::new(),
            pending: None,
            primary_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            advanced_focus: cx.focus_handle(),
            close_focus: cx.focus_handle(),
            bool_focus: HashMap::new(),
        }
    }

    /// Kick off the initial async fetch: backend schemas for add, plus the
    /// stored parameters for edit.
    fn start(&mut self, edit_name: Option<String>, cx: &mut Context<Self>) {
        let service = self.service.clone();
        self.pending = Some(cx.spawn(async move |this, cx| {
            match edit_name {
                None => {
                    let result = service.config_providers().await;
                    this.update(cx, |this, cx| {
                        match result {
                            Ok(mut providers) => {
                                providers.sort_by(|a, b| a.name.cmp(&b.name));
                                this.providers = providers;
                                this.phase = Phase::PickType;
                                this.autofocus = true;
                            }
                            Err(e) => this.error = Some(e.to_string()),
                        }
                        cx.notify();
                    })
                    .ok();
                }
                Some(name) => {
                    let providers = service.config_providers().await;
                    let stored = service.config_get(name.clone()).await;
                    this.update(cx, |this, cx| {
                        match (providers, stored) {
                            (Ok(providers), Ok(mut values)) => {
                                let kind = values
                                    .get("type")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string();
                                values.remove("type");
                                let options = providers
                                    .iter()
                                    .find(|p| p.name == kind)
                                    .map(|p| p.options.clone())
                                    .unwrap_or_default();
                                this.load_config_fields(kind, options, providers, values, cx);
                            }
                            (Err(e), _) | (_, Err(e)) => {
                                this.error = Some(e.to_string());
                                cx.notify();
                            }
                        }
                    })
                    .ok();
                }
            }
        }));
    }

    /// Backends matching the picker's search box, as `(name, description)`.
    fn filtered_backends(&self, cx: &App) -> Vec<(String, String)> {
        let query = self.search.read(cx).text().to_lowercase();
        self.providers
            .iter()
            .filter(|p| {
                query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.description.to_lowercase().contains(&query)
            })
            .map(|p| (p.name.clone(), p.description.clone()))
            .collect()
    }

    fn pick_backend(&mut self, kind: String, cx: &mut Context<Self>) {
        // Move the providers out (load_config_fields stores them back) rather than clone.
        let options = self
            .providers
            .iter()
            .find(|p| p.name == kind)
            .map(|p| p.options.clone())
            .unwrap_or_default();
        let providers = std::mem::take(&mut self.providers);
        self.load_config_fields(kind, options, providers, Map::new(), cx);
    }

    /// Build per-field inputs for the chosen backend and show the form.
    fn load_config_fields(
        &mut self,
        kind: String,
        options: Vec<RemoteOption>,
        providers: Vec<Provider>,
        stored: Map<String, Value>,
        cx: &mut Context<Self>,
    ) {
        let mut fields = HashMap::new();
        let mut bools = Map::new();
        let mut bool_focus = HashMap::new();
        for opt in &options {
            let stored_val = stored.get(&opt.name).and_then(Value::as_str).unwrap_or_default();
            if opt.kind == "bool" {
                let v = if stored_val.is_empty() { opt.default.clone() } else { stored_val.to_string() };
                bools.insert(opt.name.clone(), Value::String(v));
                bool_focus.insert(opt.name.clone(), cx.focus_handle());
            } else {
                // Don't prefill secrets (config/get returns them obscured).
                let initial = if opt.is_password { "" } else { stored_val };
                let input =
                    cx.new(|cx| TextInput::new(cx, opt.default.clone()).masked(opt.is_password));
                if !initial.is_empty() {
                    input.update(cx, |i, cx| i.set_text(initial.to_string(), cx));
                }
                fields.insert(opt.name.clone(), input);
            }
        }
        self.kind = kind;
        self.options = options;
        self.providers = providers;
        self.fields = fields;
        self.bools = bools;
        self.bool_focus = bool_focus;
        self.phase = Phase::Form;
        self.autofocus = true;
        cx.notify();
    }

    /// Validate the form, setting per-field errors. Returns true when valid.
    /// Expands the advanced section if it hides an invalid field.
    fn validate_config(&mut self, cx: &mut Context<Self>) -> bool {
        let name = self.name.read(cx).text().trim().to_string();
        let name_err: Option<SharedString> = if name.is_empty() {
            Some("Name is required".into())
        } else if self.mode == ConfigMode::Add && self.remote_names.iter().any(|n| n == &name) {
            Some(format!("A remote named \"{name}\" already exists").into())
        } else {
            None
        };

        // (entity, error, advanced) for every text field — clears stale errors too.
        let mut fields: Vec<(Entity<TextInput>, Option<SharedString>, bool)> = Vec::new();
        for opt in self.options.iter().filter(|o| o.kind != "bool") {
            if let Some(input) = self.fields.get(&opt.name) {
                let err = (opt.required && input.read(cx).text().trim().is_empty())
                    .then(|| SharedString::from("This field is required"));
                fields.push((input.clone(), err, opt.advanced));
            }
        }

        self.name.update(cx, |i, cx| i.set_error(name_err.clone(), cx));
        let mut valid = name_err.is_none();
        for (input, err, advanced) in fields {
            if err.is_some() {
                valid = false;
                self.show_advanced |= advanced;
            }
            input.update(cx, |i, cx| i.set_error(err, cx));
        }
        valid
    }

    fn submit_config(&mut self, cx: &mut Context<Self>) {
        if !self.validate_config(cx) {
            cx.notify();
            return;
        }
        let name = self.name.read(cx).text().trim().to_string();
        let mut params = Map::new();
        for opt in &self.options {
            let value = if opt.kind == "bool" {
                self.bools.get(&opt.name).and_then(Value::as_str).unwrap_or_default().to_string()
            } else {
                self.fields.get(&opt.name).map(|i| i.read(cx).text().to_string()).unwrap_or_default()
            };
            if !value.is_empty() {
                params.insert(opt.name.clone(), Value::String(value));
            }
        }
        let (mode, kind) = (self.mode, self.kind.clone());
        self.phase = Phase::Busy;
        self.error = None;
        let opt = serde_json::json!({ "nonInteractive": true, "obscure": true });
        self.run_config_step(mode, name, kind, Value::Object(params), opt, cx);
    }

    fn answer_question(&mut self, cx: &mut Context<Self>) {
        let (mode, name, kind, state) = (
            self.mode,
            self.name.read(cx).text().to_string(),
            self.kind.clone(),
            self.state.clone(),
        );
        let answer = self.answer.read(cx).text().to_string();
        self.phase = Phase::Busy;
        self.error = None;
        let opt = serde_json::json!({
            "nonInteractive": true, "obscure": true, "continue": true, "state": state, "result": answer,
        });
        self.run_config_step(mode, name, kind, Value::Object(Map::new()), opt, cx);
    }

    fn run_config_step(
        &mut self,
        mode: ConfigMode,
        name: String,
        kind: String,
        params: Value,
        opt: Value,
        cx: &mut Context<Self>,
    ) {
        let service = self.service.clone();
        self.pending = Some(cx.spawn(async move |this, cx| {
            let step = match mode {
                ConfigMode::Add => service.config_create(name, kind, params, opt).await,
                ConfigMode::Edit => service.config_update(name, params, opt).await,
            };
            this.update(cx, |this, cx| match step {
                Ok(step) => this.on_config_step(step, cx),
                Err(e) => {
                    this.error = Some(e.to_string());
                    this.phase = this.error_phase();
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn on_config_step(&mut self, step: rspace_rclone_rc::ConfigStep, cx: &mut Context<Self>) {
        if !step.state.is_empty() {
            if let Some(question) = step.option {
                let default = question.default.clone();
                self.question = Some(question);
                self.state = step.state;
                self.phase = Phase::Question;
                self.error = (!step.error.is_empty()).then(|| step.error.clone());
                self.autofocus = true;
                let answer = self.answer.clone();
                answer.update(cx, |i, cx| i.set_text(default, cx));
            } else {
                self.error = Some("unexpected config state".into());
                self.phase = Phase::Form;
            }
            cx.notify();
        } else if !step.error.is_empty() {
            self.error = Some(step.error);
            self.phase = self.error_phase();
            cx.notify();
        } else {
            cx.emit(RemoteConfigEvent::Saved);
        }
    }

    /// Phase a recoverable error returns to (a mid-flow question, else the form).
    fn error_phase(&self) -> Phase {
        if self.question.is_some() { Phase::Question } else { Phase::Form }
    }

    fn toggle_advanced(&mut self, cx: &mut Context<Self>) {
        self.show_advanced = !self.show_advanced;
        cx.notify();
    }

    fn set_bool(&mut self, name: String, on: bool, cx: &mut Context<Self>) {
        self.bools.insert(name, Value::String(if on { "true" } else { "false" }.into()));
        cx.notify();
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(RemoteConfigEvent::Dismiss);
    }

    /// In flight: an OAuth backend may have its auth webserver up (stop it on close).
    pub(crate) fn is_busy(&self) -> bool {
        self.phase == Phase::Busy
    }

    /// Focus the phase's primary input once after opening or advancing a step.
    fn focus_primary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.autofocus {
            return;
        }
        self.autofocus = false;
        let input = match self.phase {
            Phase::PickType => Some(self.search.clone()),
            Phase::Question => Some(self.answer.clone()),
            Phase::Form if self.mode == ConfigMode::Add => Some(self.name.clone()),
            _ => None,
        };
        match input {
            Some(input) => input.read(cx).focus_handle(cx).focus(window, cx),
            // Keep focus inside the modal (so its shortcuts dispatch) even when no
            // input is the natural target (e.g. the Busy phase, an edit form).
            None => self.focus_handle.focus(window, cx),
        }
    }
}

impl Workspace {
    pub(crate) fn begin_add_remote(&mut self, cx: &mut Context<Self>) {
        let names = self.remotes.iter().map(|r| r.name.clone()).collect();
        let service = self.service.clone();
        let modal = cx.new(|cx| {
            let mut m = RemoteConfigModal::new(ConfigMode::Add, None, service, names, cx);
            m.start(None, cx);
            m
        });
        self.attach_remote_config(modal, cx);
    }

    pub(crate) fn begin_edit_remote(&mut self, name: String, cx: &mut Context<Self>) {
        let names = self.remotes.iter().map(|r| r.name.clone()).collect();
        let service = self.service.clone();
        let modal = cx.new(|cx| {
            let mut m = RemoteConfigModal::new(ConfigMode::Edit, Some(&name), service, names, cx);
            m.start(Some(name), cx);
            m
        });
        self.attach_remote_config(modal, cx);
    }

    fn attach_remote_config(
        &mut self,
        modal: Entity<RemoteConfigModal>,
        cx: &mut Context<Self>,
    ) {
        self.remote_config_sub = Some(cx.subscribe(&modal, |this, _, event, cx| match event {
            RemoteConfigEvent::Saved => {
                this.remote_config = None;
                this.load_remotes(cx);
            }
            RemoteConfigEvent::Dismiss => this.close_remote_config(cx),
        }));
        self.remote_config = Some(modal);
        cx.notify();
    }

    /// Close the modal. If a config step was in flight, also stop rclone's OAuth
    /// webserver so an abandoned interactive auth doesn't keep its port bound.
    pub(crate) fn close_remote_config(&mut self, cx: &mut Context<Self>) {
        if let Some(modal) = self.remote_config.take() {
            if modal.read(cx).is_busy() {
                let service = self.service.clone();
                cx.spawn(async move |_, _| {
                    let _ = service.config_oauth_stop().await;
                })
                .detach();
            }
        }
        cx.notify();
    }
}

mod nav;
mod view;
