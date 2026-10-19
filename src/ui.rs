use crate::schedule_demo;
use gpui::assets::IconName;
use gpui::component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    input::{
        Input, InputEvent, InputGroup, InputGroupAddon, InputGroupAddonAlignment, InputGroupButton,
        InputState,
    },
    spinner::Spinner,
    switch::Switch,
};
use gpui::{
    Context, Div, Entity, Focusable, FontWeight, IntoElement, ScrollHandle, Window, div,
    prelude::*, px, rgb,
};
use std::{sync::mpsc, time::Duration};
use win_notifi_toast::chat::{ChatSession, Connection, Provider};
use win_notifi_toast::{
    commands::CommandRecovery,
    reminder_tools::{CommandOutcome, prepare_command, resume_command_with_progress},
    reminders::{CommandContext, ReminderId},
};

const PANEL_HEADER_HEIGHT: f32 = 52.;

#[derive(IntoElement)]
struct TextLineIcon(Icon);

impl gpui::RenderOnce for TextLineIcon {
    fn render(self, window: &mut Window, _: &mut gpui::App) -> impl IntoElement {
        // Center on the first text line, even when the adjacent text wraps.
        div()
            .h(window.line_height())
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(self.0)
    }
}

enum ChatEvent {
    Fact(String),
    Finished(Result<CommandOutcome, String>),
}
enum ListAction {
    Pause(ReminderId, bool),
    Delete(ReminderId),
    Undo,
}

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Reminders,
    Settings,
}
#[derive(Clone, Copy, PartialEq)]
enum Filter {
    Active,
    Paused,
    Completed,
}

pub struct MainWindow {
    pub session: Option<ChatSession>,
    pub command: Entity<InputState>,
    base_url: Entity<InputState>,
    api_key: Entity<InputState>,
    model: Entity<InputState>,
    pub status: String,
    pub busy: bool,
    verified_connection: Option<Connection>,
    pub(crate) selected_id: Option<ReminderId>,
    pending: Option<CommandRecovery>,
    page: Page,
    filter: Filter,
    history_scroll: ScrollHandle,
}
impl MainWindow {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let opened = ChatSession::open(ChatSession::default_directory());
        let status = opened
            .as_ref()
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        let session = opened.ok();
        let connection = session
            .as_ref()
            .map(|s| s.connection().clone())
            .unwrap_or_default();
        let pending = session.as_ref().and_then(|s| s.pending()).cloned();
        let selected_id = pending.as_ref().and_then(|p| p.context.selected_id);
        let status = if status.is_empty() && pending.is_some() {
            "Есть незавершённый ответ. Его можно продолжить без повторного выполнения операций."
                .into()
        } else {
            status
        };
        let command = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Например: напомни выпить воды через 20 минут")
        });
        cx.subscribe_in(&command, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.request(false, window, cx);
            }
        })
        .detach();
        let base_url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://api.openai.com/v1")
                .default_value(connection.base_url)
        });
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Ваш API-ключ")
                .default_value(connection.api_key)
                .masked(true)
        });
        let model = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("ID модели")
                .default_value(connection.model)
        });
        for input in [&base_url, &api_key, &model] {
            cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.save_connection(cx);
                }
            })
            .detach();
        }
        let history_scroll = ScrollHandle::new();
        history_scroll.scroll_to_bottom();
        Self {
            session,
            command,
            base_url,
            api_key,
            model,
            status,
            busy: false,
            verified_connection: None,
            selected_id,
            pending,
            page: Page::Reminders,
            filter: Filter::Active,
            history_scroll,
        }
    }
    fn apply_list_action(&mut self, action: ListAction, cx: &mut Context<Self>) {
        if self.busy {
            self.status = "Дождитесь завершения текущей команды".into();
            cx.notify();
            return;
        }
        let application = cx.global::<crate::reminder_ui::Reminders>().app.clone();
        let result = (|| -> anyhow::Result<String> {
            let mut app = application
                .lock()
                .map_err(|_| anyhow::anyhow!("Ядро напоминаний недоступно"))?;
            match action {
                ListAction::Pause(id, paused) => {
                    app.set_paused(id, paused)?;
                    Ok(format!(
                        "{} #{}",
                        if paused {
                            "Приостановлено"
                        } else {
                            "Возобновлено"
                        },
                        id
                    ))
                }
                ListAction::Delete(id) => {
                    let reminder = app.delete(id)?;
                    if self.selected_id == Some(id) {
                        self.selected_id = None;
                    }
                    Ok(format!("Удалено #{}: «{}»", id, reminder.text))
                }
                ListAction::Undo => {
                    let restored = app.undo()?;
                    Ok(restored.map_or_else(
                        || "Отмена выполнена. Созданная запись удалена".into(),
                        |r| format!("Отмена выполнена. Восстановлено #{}: «{}»", r.id, r.text),
                    ))
                }
            }
        })();
        self.status = match result {
            Ok(fact) => {
                if let Some(session) = self.session.as_mut()
                    && let Err(error) = session.record_action(uuid::Uuid::new_v4(), &fact)
                {
                    format!("{fact}. Не удалось сохранить историю: {error}")
                } else {
                    String::new()
                }
            }
            Err(error) => error.to_string(),
        };
        self.history_scroll.scroll_to_bottom();
        cx.notify();
    }
    fn open_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.save_connection(cx) {
            crate::model_picker::open(
                self.session.as_ref().unwrap().connection().clone(),
                cx.entity(),
                window,
                cx,
            );
        }
    }
    pub(crate) fn select_catalog_model(
        &mut self,
        catalog: &win_notifi_toast::catalog::Catalog,
        id: &str,
        source: &Connection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Локальное хранилище недоступно"))?;
        if session.connection().base_url != source.base_url
            || session.connection().api_key != source.api_key
        {
            anyhow::bail!("Подключение изменилось. Откройте каталог заново.");
        }
        catalog.select(id, session)?;
        self.model
            .update(cx, |model, cx| model.set_value(id.to_owned(), window, cx));
        self.status = format!("Выбрана модель {id}; перед запросом проверим инструменты");
        cx.notify();
        Ok(())
    }
    fn save_connection(&mut self, cx: &mut Context<Self>) -> bool {
        let connection = Connection {
            base_url: self.base_url.read(cx).value().trim().into(),
            api_key: self.api_key.read(cx).value().trim().into(),
            model: self.model.read(cx).value().trim().into(),
        };
        match self
            .session
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Локальное хранилище недоступно"))
            .and_then(|s| s.save_connection(connection))
        {
            Ok(()) => {
                self.status.clear();
                cx.notify();
                true
            }
            Err(e) => {
                self.status = e.to_string();
                cx.notify();
                false
            }
        }
    }
    fn request(&mut self, probe: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.send_request(probe, false, window, cx);
    }
    fn retry_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            self.send_request(false, true, window, cx);
        }
    }
    fn send_request(
        &mut self,
        probe: bool,
        retry: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if !self.save_connection(cx) {
            if probe {
                show_probe_result(std::mem::take(&mut self.status), false, window, cx);
            }
            return;
        }
        let provider = match Provider::new(self.session.as_ref().unwrap().connection()) {
            Ok(p) => p,
            Err(e) => {
                self.status = e.to_string();
                if probe {
                    show_probe_result(std::mem::take(&mut self.status), false, window, cx);
                }
                cx.notify();
                return;
            }
        };
        let mut context = None;
        let mut recovery = if retry { self.pending.clone() } else { None };
        let application = cx.global::<crate::reminder_ui::Reminders>().app.clone();
        if !probe && !retry {
            let text = self.command.read(cx).value().trim().to_owned();
            if text.is_empty() {
                return;
            }
            context = Some(CommandContext::capture(
                application.lock().unwrap().clock(),
                &text,
                self.selected_id,
            ));
            if let Err(e) = self.session.as_mut().unwrap().record_user(&text) {
                self.status = e.to_string();
                cx.notify();
                return;
            }
            self.command
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
        let history = self.session.as_ref().unwrap().messages().to_vec();
        if !probe {
            if recovery.is_none() {
                recovery = match prepare_command(&history, context.unwrap()) {
                    Ok(recovery) => Some(recovery),
                    Err(error) => {
                        self.status = error.to_string();
                        cx.notify();
                        return;
                    }
                };
            }
            if let Err(error) = self
                .session
                .as_mut()
                .unwrap()
                .save_pending(recovery.clone())
            {
                self.status = error.to_string();
                cx.notify();
                return;
            }
            self.pending = recovery.clone();
            self.history_scroll.scroll_to_bottom();
        }
        let connection = self.session.as_ref().unwrap().connection().clone();
        let must_probe = self.verified_connection.as_ref() != Some(&connection);
        let command_id = recovery.as_ref().map(|r| r.context.id);
        self.busy = true;
        self.status = if probe {
            "Проверяем подключение и инструменты…"
        } else {
            ""
        }
        .into();
        cx.notify();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Не удалось запустить сетевой обработчик".to_owned())
                .and_then(|rt| {
                    rt.block_on(async {
                        if probe {
                            provider.probe().await.map(|_| CommandOutcome {
                                text: "Подключение и tool calling работают".into(),
                                ..Default::default()
                            })
                        } else {
                            if must_probe {
                                provider.probe().await?;
                            }
                            let progress = sender.clone();
                            Ok(resume_command_with_progress(
                                &provider,
                                recovery.unwrap(),
                                application,
                                move |fact| {
                                    let _ = progress.send(ChatEvent::Fact(fact.into()));
                                },
                            )
                            .await)
                        }
                    })
                    .map_err(|e| e.to_string())
                });
            let _ = sender.send(ChatEvent::Finished(result));
        });
        let request_window = window.window_handle();
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                match receiver.try_recv() {
                    Ok(ChatEvent::Fact(fact)) => {
                        let _ = view.update(cx, |view, cx| {
                            if let Some(session) = view.session.as_mut()
                                && let Err(error) = session.record_action(command_id.unwrap(), &fact)
                            {
                                view.status = error.to_string();
                            }
                            view.history_scroll.scroll_to_bottom();
                            cx.notify();
                        });
                    }
                    Ok(ChatEvent::Finished(result)) => {
                        let success = result.as_ref().is_ok_and(|outcome| outcome.error.is_none());
                        let message = view.update(cx, |view, cx| {
                            view.busy = false;
                            view.history_scroll.scroll_to_bottom();
                            match result {
                                Ok(outcome) => {
                                    view.verified_connection = Some(connection);
                                    let recovery_error=if !probe {
                                        view.pending=outcome.recovery.clone();
                                        view.session.as_mut().and_then(|s|s.save_pending(outcome.recovery).err()).map(|e|e.to_string())
                                    } else {None};
                                    let saved = !outcome.facts.is_empty();
                                    if saved { view.selected_id=None; }
                                    let text = outcome.text;
                                    if probe {
                                        view.status = text.clone();
                                    } else if let Some(session) = view.session.as_mut()
                                        && !text.is_empty()
                                    {
                                        view.status = match session.finish_reply(command_id.unwrap(), &text) {
                                            Ok(()) => String::new(),
                                            Err(e) => e.to_string(),
                                        };
                                    }
                                    if let Some(error) = outcome.error {
                                        view.status = if saved {
                                            format!("{error} Фактический результат операции сохранён в чате.")
                                        } else {
                                            error
                                        };
                                    }
                                    if let Some(error)=recovery_error {view.status=format!("{} Не удалось сохранить продолжение ответа: {error}",view.status);}
                                }
                                Err(e) => view.status = if !probe && view.pending.as_ref().is_some_and(|p|!p.facts.is_empty()) {format!("{e} Ранее выполненные изменения сохранены.")} else {e},
                            }
                            cx.notify();
                            if probe { std::mem::take(&mut view.status) } else { String::new() }
                        });
                        if probe && let Ok(message) = message {
                            let _ = request_window.update(cx, |_, window, cx| {
                                show_probe_result(message, success, window, cx);
                            });
                        }
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let message = view.update(cx, |view, cx| {
                            view.busy = false;
                            view.history_scroll.scroll_to_bottom();
                            view.status = "Сетевой обработчик завершился без ответа".into();
                            cx.notify();
                            if probe { std::mem::take(&mut view.status) } else { String::new() }
                        });
                        if probe && let Ok(message) = message {
                            let _ = request_window.update(cx, |_, window, cx| {
                                show_probe_result(message, false, window, cx);
                            });
                        }
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }
}
impl MainWindow {
    fn connection_settings(&self, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let t = cx.theme().clone();
        div()
            .id("settings-content")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .child(
                div()
                    .max_w(px(720.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.background)
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Подключение"))
                            .child(field(
                                "Базовый URL",
                                Input::new(&self.base_url).disabled(self.busy),
                            ))
                            .child(field(
                                "API-ключ",
                                Input::new(&self.api_key).disabled(self.busy),
                            ))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(t.muted_foreground)
                                    .child("Ключ хранится локально открытым текстом."),
                            )
                            .child(field(
                                "Модель",
                                InputGroup::new("model-input")
                                    .input(Input::new(&self.model))
                                    .disabled(self.busy)
                                    .addon(
                                        InputGroupAddon::new("model-picker")
                                            .align(InputGroupAddonAlignment::InlineEnd)
                                            .child(
                                                InputGroupButton::new("catalog")
                                                    .label("Выбрать…")
                                                    .on_click(cx.listener(|v, _, window, cx| {
                                                        v.open_catalog(window, cx)
                                                    })),
                                            ),
                                    ),
                            ))
                            .child(
                                div().flex().flex_wrap().gap_2().child(
                                    Button::new("probe")
                                        .icon(IconName::Plug)
                                        .label("Проверить")
                                        .loading(self.busy)
                                        .on_click(cx.listener(|v, _, window, cx| {
                                            v.request(true, window, cx)
                                        })),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.background)
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Тосты"))
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Button::new("placement")
                                            .icon(IconName::Settings2)
                                            .label("Размещение тостов")
                                            .on_click(|_, w, cx| crate::settings_ui::open(w, cx)),
                                    )
                                    .child(
                                        Button::new("demo")
                                            .icon(IconName::FlaskConical)
                                            .label("Тест тостов")
                                            .on_click(|_, _, cx| schedule_demo(cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(t.border)
                            .bg(t.background)
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(div().flex_1().child("Автозапуск при входе в Windows"))
                            .child(
                                Switch::new("autostart")
                                    .accessibility_label("Автозапуск при входе в Windows")
                                    .checked(
                                        cx.global::<crate::settings_ui::DesktopSettings>()
                                            .0
                                            .settings()
                                            .autostart,
                                    )
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        let service = &mut cx
                                            .global_mut::<crate::settings_ui::DesktopSettings>()
                                            .0;
                                        let mut next = service.settings().clone();
                                        next.autostart = *checked;
                                        this.status = match service.save(next) {
                                            Ok(()) => String::new(),
                                            Err(error) => error.to_string(),
                                        };
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
    }

    fn workspace(&self, cx: &mut Context<Self>) -> Div {
        let t = cx.theme().clone();
        let records = cx
            .global::<crate::reminder_ui::Reminders>()
            .app
            .lock()
            .map(|app| {
                app.reminders()
                    .iter()
                    .map(|r| (r.clone(), r.schedule.description(app.clock())))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let counts = [
            records
                .iter()
                .filter(|(r, _)| !r.completed && !r.paused)
                .count(),
            records
                .iter()
                .filter(|(r, _)| !r.completed && r.paused)
                .count(),
            records.iter().filter(|(r, _)| r.completed).count(),
        ];
        let mut filters = div().flex().flex_wrap().gap_1();
        for (index, (filter, label)) in [
            (Filter::Active, "Активные"),
            (Filter::Paused, "На паузе"),
            (Filter::Completed, "Завершённые"),
        ]
        .into_iter()
        .enumerate()
        {
            filters = filters.child(
                Button::new(("filter", index))
                    .icon(match filter {
                        Filter::Active => IconName::Bell,
                        Filter::Paused => IconName::Pause,
                        Filter::Completed => IconName::Check,
                    })
                    .ghost()
                    .small()
                    .selected(self.filter == filter)
                    .label(label)
                    .accessibility_label(format!("{label}  {}", counts[index]))
                    .child(
                        div()
                            .font_weight(if counts[index] > 0 {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(counts[index].to_string()),
                    )
                    .on_click(cx.listener(move |v, _, _, cx| {
                        v.filter = filter;
                        cx.notify();
                    })),
            );
        }
        let mut list = div()
            .id("reminder-list")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3();
        let visible = records
            .iter()
            .filter(|(r, _)| match self.filter {
                Filter::Active => !r.completed && !r.paused,
                Filter::Paused => !r.completed && r.paused,
                Filter::Completed => r.completed,
            })
            .collect::<Vec<_>>();
        if visible.is_empty() {
            list = list.child(div().py_6().text_sm().text_color(t.muted_foreground).child(
                match self.filter {
                    Filter::Active => "Нет активных напоминаний",
                    Filter::Paused => "Нет напоминаний на паузе",
                    Filter::Completed => "Нет завершённых напоминаний",
                },
            ));
        }
        for (r, description) in visible {
            let id = r.id;
            let paused = r.paused;
            let selected = self.selected_id == Some(id);
            list = list.child(
                div()
                    .id(("reminder", id as usize))
                    .flex_shrink_0()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(if selected { t.primary } else { t.border })
                    .bg(t.background)
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .items_start()
                            .child(div().w(px(4.)).h(px(36.)).rounded_full().bg(rgb(r.border)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(r.text.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .items_start()
                                            .text_sm()
                                            .text_color(t.muted_foreground)
                                            .child(TextLineIcon(Icon::new(IconName::Clock).small()))
                                            .child(
                                                div().flex_1().min_w_0().child(description.clone()),
                                            ),
                                    )
                                    .when(r.sound, |e| {
                                        e.child(
                                            div()
                                                .text_xs()
                                                .text_color(t.muted_foreground)
                                                .child("Со звуком"),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(
                                Button::new("edit")
                                    .ghost()
                                    .small()
                                    .icon(IconName::SquarePen)
                                    .label("Изменить")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |v, _, window, cx| {
                                        v.selected_id = Some(id);
                                        v.status.clear();
                                        v.command.read(cx).focus_handle(cx).focus(window, cx);
                                        cx.notify();
                                    })),
                            )
                            .when(!r.completed, |e| {
                                e.child(
                                    Button::new("pause")
                                        .ghost()
                                        .small()
                                        .icon(if paused {
                                            IconName::Play
                                        } else {
                                            IconName::Pause
                                        })
                                        .label(if paused {
                                            "Возобновить"
                                        } else {
                                            "На паузу"
                                        })
                                        .disabled(self.busy)
                                        .on_click(cx.listener(move |v, _, _, cx| {
                                            v.apply_list_action(ListAction::Pause(id, !paused), cx)
                                        })),
                                )
                            })
                            .child(
                                Button::new("delete")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Trash)
                                    .tooltip("Удалить напоминание")
                                    .accessibility_label("Удалить напоминание")
                                    .disabled(self.busy)
                                    .on_click(cx.listener(move |v, _, _, cx| {
                                        v.apply_list_action(ListAction::Delete(id), cx)
                                    })),
                            ),
                    ),
            );
        }
        let reminders =
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .min_h_0()
                .rounded_xl()
                .border_1()
                .border_color(t.border)
                .bg(t.background)
                .child(
                    div()
                        .px_3()
                        .h(px(PANEL_HEADER_HEIGHT))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .border_b_1()
                        .border_color(t.border)
                        .child(filters)
                        .child(
                            Button::new("undo")
                                .ghost()
                                .small()
                                .icon(IconName::Undo2)
                                .tooltip("Отменить последнее изменение")
                                .accessibility_label("Отменить последнее изменение")
                                .disabled(self.busy)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    v.apply_list_action(ListAction::Undo, cx)
                                })),
                        ),
                )
                .child(list);
        let messages = self
            .session
            .as_ref()
            .map(|s| s.messages().to_vec())
            .unwrap_or_default();
        let mut history = div()
            .id("chat-history")
            .track_scroll(&self.history_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3();
        for (index, m) in messages.into_iter().enumerate() {
            let user = m.role == "user";
            let action = m.result_for.is_some();
            history = history.child(
                div()
                    .id(("message", index))
                    .when(user, |e| e.ml_6())
                    .when(!user, |e| e.mr_6())
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(if action {
                                t.success.opacity(0.08)
                            } else if user {
                                t.secondary
                            } else {
                                t.background
                            })
                            .border_1()
                            .border_color(if action {
                                t.success.opacity(0.5)
                            } else if user {
                                t.secondary
                            } else {
                                t.border
                            })
                            .text_sm()
                            .when(action, |e| {
                                e.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .mb_2()
                                        .text_color(t.success)
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(Icon::new(IconName::CircleCheck).small())
                                        .child("Выполнено"),
                                )
                            })
                            .child(m.content),
                    ),
            );
        }
        if self.busy {
            history = history.child(
                div()
                    .id("assistant-pending")
                    .mr_6()
                    .flex_shrink_0()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(t.muted_foreground)
                    .child(Spinner::new().small())
                    .child("Ассистент готовит ответ…"),
            );
        }
        if !self.status.is_empty() {
            history = history.child(
                div()
                    .id("chat-status")
                    .flex_shrink_0()
                    .p_3()
                    .rounded_lg()
                    .bg(t.secondary)
                    .text_sm()
                    .child(self.status.clone()),
            );
        }
        let configured = self
            .session
            .as_ref()
            .is_some_and(|s| !s.connection().model.is_empty());
        let chat =
            div()
                .flex_1()
                .flex_grow(1.5)
                .min_w_0()
                .flex()
                .flex_col()
                .min_h_0()
                .rounded_xl()
                .border_1()
                .border_color(t.border)
                .bg(t.background)
                .child(
                    div()
                        .px_3()
                        .h(px(PANEL_HEADER_HEIGHT))
                        .flex_shrink_0()
                        .border_b_1()
                        .border_color(t.border)
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Чат"))
                        .child(
                            Button::new("clear")
                                .ghost()
                                .small()
                                .icon(IconName::Eraser)
                                .tooltip("Очистить историю")
                                .accessibility_label("Очистить историю")
                                .disabled(self.busy)
                                .on_click(cx.listener(|v, _, _, cx| {
                                    if let Some(s) = v.session.as_mut() {
                                        match s.clear() {
                                            Ok(()) => {
                                                v.selected_id = None;
                                                v.pending = None;
                                                v.status.clear();
                                            }
                                            Err(e) => v.status = e.to_string(),
                                        }
                                    }
                                    cx.notify();
                                })),
                        ),
                )
                .child(history)
                .child(
                    div()
                        .p_3()
                        .border_t_1()
                        .border_color(t.border)
                        .flex()
                        .flex_col()
                        .gap_3()
                        .when(!configured, |e| {
                            e.child(
                                Button::new("setup")
                                    .label("Подключить ассистента")
                                    .icon(IconName::Settings2)
                                    .w_full()
                                    .on_click(cx.listener(|v, _, _, cx| {
                                        v.page = Page::Settings;
                                        cx.notify();
                                    })),
                            )
                        })
                        .when_some(self.selected_id, |e, id| {
                            e.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_sm()
                                    .text_color(t.primary)
                                    .child(format!("Изменение № {id}"))
                                    .child(
                                        Button::new("detach")
                                            .ghost()
                                            .small()
                                            .icon(IconName::X)
                                            .tooltip("Снять выбор")
                                            .accessibility_label("Снять выбор")
                                            .disabled(self.busy)
                                            .on_click(cx.listener(|v, _, _, cx| {
                                                v.selected_id = None;
                                                cx.notify();
                                            })),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .child(Input::new(&self.command).disabled(self.busy)),
                                )
                                .child(
                                    Button::new("send")
                                        .primary()
                                        .icon(IconName::ArrowUp)
                                        .tooltip("Отправить · Enter")
                                        .accessibility_label("Отправить")
                                        .loading(self.busy)
                                        .on_click(cx.listener(|v, _, window, cx| {
                                            v.request(false, window, cx)
                                        })),
                                ),
                        )
                        .when(self.pending.is_some() && !self.busy, |e| {
                            e.child(
                                Button::new("retry")
                                    .icon(IconName::RefreshCw)
                                    .label("Продолжить ответ")
                                    .disabled(self.busy)
                                    .w_full()
                                    .on_click(
                                        cx.listener(|v, _, window, cx| v.retry_command(window, cx)),
                                    ),
                            )
                        }),
                );
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .gap_3()
            .p_3()
            .child(reminders)
            .child(chat)
    }
}

impl Render for MainWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI")
            .text_base()
            .text_color(t.foreground)
            .bg(t.muted)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .border_b_1()
                    .border_color(t.border)
                    .bg(t.background)
                    .child(
                        Button::new("nav-reminders")
                            .icon(IconName::Bell)
                            .ghost()
                            .selected(self.page == Page::Reminders)
                            .label("Напоминания")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.page = Page::Reminders;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("nav-settings")
                            .icon(IconName::Settings2)
                            .ghost()
                            .selected(self.page == Page::Settings)
                            .label("Настройки")
                            .on_click(cx.listener(|v, _, _, cx| {
                                v.page = Page::Settings;
                                cx.notify();
                            })),
                    ),
            )
            .child(match self.page {
                Page::Reminders => self.workspace(cx).into_any_element(),
                Page::Settings => self.connection_settings(cx).into_any_element(),
            })
            .when(
                self.page == Page::Settings && !self.status.is_empty(),
                |e| {
                    e.child(
                        div()
                            .id("status")
                            .max_h(px(72.))
                            .overflow_y_scroll()
                            .px_3()
                            .py_2()
                            .bg(t.secondary)
                            .text_sm()
                            .child(self.status.clone()),
                    )
                },
            )
    }
}

fn field(label: &'static str, input: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
        .child(input)
}

pub fn error_message(message: String) -> Div {
    div().p_3().font_family("Segoe UI").child(message)
}

fn show_probe_result(message: String, success: bool, window: &mut Window, cx: &mut gpui::App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme();
        dialog
            .title(if success {
                "Подключение работает"
            } else {
                "Не удалось проверить подключение"
            })
            .width(px(520.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(TextLineIcon(
                        Icon::new(if success {
                            IconName::CircleCheck
                        } else {
                            IconName::CircleAlert
                        })
                        .text_color(if success {
                            theme.success
                        } else {
                            theme.danger
                        }),
                    ))
                    .child(div().flex_1().min_w_0().child(message.clone())),
            )
            .footer(
                Button::new("close-probe-result")
                    .icon(IconName::Check)
                    .label("Понятно")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
    });
}
