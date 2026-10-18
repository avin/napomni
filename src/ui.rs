use crate::{input::TextInput, schedule_demo};
use gpui::{
    App, Context, Div, Entity, IntoElement, Window, WindowAppearance, div, prelude::*, rgb,
};
use std::{sync::mpsc, time::Duration};
use win_notifi_toast::chat::{ChatSession, Connection, Provider};

struct Palette {
    background: u32,
    surface: u32,
    text: u32,
    muted: u32,
    border: u32,
}
impl Palette {
    fn for_window(window: &Window) -> Self {
        match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self {
                background: 0x15171b,
                surface: 0x202329,
                text: 0xf3f4f6,
                muted: 0xb1b8c4,
                border: 0x393e48,
            },
            _ => Self {
                background: 0xf5f6f8,
                surface: 0xffffff,
                text: 0x20242c,
                muted: 0x5e6878,
                border: 0xdce0e7,
            },
        }
    }
}
fn button(
    label: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(label)
        .px_3()
        .py_2()
        .bg(rgb(0x365edc))
        .text_color(rgb(0xffffff))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x284ab9)))
        .on_click(move |_, w, c| on_click(w, c))
        .child(label)
}
pub struct MainWindow {
    pub session: Option<ChatSession>,
    pub command: Entity<TextInput>,
    base_url: Entity<TextInput>,
    api_key: Entity<TextInput>,
    model: Entity<TextInput>,
    pub status: String,
    pub busy: bool,
    verified_connection: Option<Connection>,
}
impl MainWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
        Self {
            session,
            command: cx.new(|cx| TextInput::new("", "Введите команду по-русски", cx)),
            base_url: cx.new(|cx| TextInput::new(connection.base_url, "Базовый URL", cx)),
            api_key: cx.new(|cx| TextInput::new(connection.api_key, "API-ключ", cx)),
            model: cx.new(|cx| TextInput::new(connection.model, "ID модели", cx)),
            status,
            busy: false,
            verified_connection: None,
        }
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
                self.status = "Настройки подключения сохранены".into();
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
    fn request(&mut self, probe: bool, cx: &mut Context<Self>) {
        if self.busy || !self.save_connection(cx) {
            return;
        }
        let provider = match Provider::new(self.session.as_ref().unwrap().connection()) {
            Ok(p) => p,
            Err(e) => {
                self.status = e.to_string();
                cx.notify();
                return;
            }
        };
        if !probe {
            let text = self.command.read(cx).value().trim().to_owned();
            if text.is_empty() {
                return;
            }
            if let Err(e) = self.session.as_mut().unwrap().record_user(&text) {
                self.status = e.to_string();
                cx.notify();
                return;
            }
            self.command.update(cx, |input, cx| input.set_value("", cx));
        }
        let history = self.session.as_ref().unwrap().messages().to_vec();
        let connection = self.session.as_ref().unwrap().connection().clone();
        let must_probe = self.verified_connection.as_ref() != Some(&connection);
        self.busy = true;
        self.status = if probe {
            "Проверяем подключение и инструменты…"
        } else {
            "Ожидаем ответ…"
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
                            provider
                                .probe()
                                .await
                                .map(|_| "Подключение и tool calling работают".to_owned())
                        } else {
                            if must_probe {
                                provider.probe().await?;
                            }
                            provider.reply(&history).await
                        }
                    })
                    .map_err(|e| e.to_string())
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(40))
                    .await;
                match receiver.try_recv() {
                    Ok(result) => {
                        let _ = view.update(cx, |view, cx| {
                            view.busy = false;
                            match result {
                                Ok(text) => {
                                    view.verified_connection = Some(connection);
                                    if probe {
                                        view.status = text;
                                    } else if let Some(session) = view.session.as_mut() {
                                        view.status = match session.record_assistant(&text) {
                                            Ok(()) => String::new(),
                                            Err(e) => e.to_string(),
                                        };
                                    }
                                }
                                Err(e) => view.status = e,
                            }
                            cx.notify();
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        let _ = view.update(cx, |view, cx| {
                            view.busy = false;
                            view.status = "Сетевой обработчик завершился без ответа".into();
                            cx.notify();
                        });
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                }
            }
        })
        .detach();
    }
}
impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = Palette::for_window(window);
        let entity = cx.entity();
        let save = entity.clone();
        let probe = entity.clone();
        let clear = entity.clone();
        let send = entity.clone();
        let messages = self
            .session
            .as_ref()
            .map(|s| s.messages().to_vec())
            .unwrap_or_default();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .font_family("Segoe UI")
            .text_base()
            .text_color(rgb(c.text))
            .bg(rgb(c.background))
            .child(div().text_2xl().child("Напоминания"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .border_1()
                    .border_color(rgb(c.border))
                    .bg(rgb(c.surface))
                    .child("Подключение LLM")
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .child("Базовый URL")
                                    .child(self.base_url.clone()),
                            )
                            .child(div().flex_1().child("ID модели").child(self.model.clone())),
                    )
                    .child(
                        div()
                            .child("API-ключ (сохраняется открытым текстом)")
                            .child(self.api_key.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(button(
                                "Сохранить подключение",
                                move |_, cx| {
                                    save.update(cx, |v, cx| {
                                        v.save_connection(cx);
                                    })
                                },
                            ))
                            .child(button(
                                "Проверить подключение",
                                move |_, cx| probe.update(cx, |v, cx| v.request(true, cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(gpui::px(240.))
                            .p_3()
                            .bg(rgb(c.surface))
                            .child("Список напоминаний")
                            .child(
                                div()
                                    .mt_3()
                                    .text_color(rgb(c.muted))
                                    .child("Пока напоминаний нет"),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .min_h_0()
                            .gap_2()
                            .p_3()
                            .bg(rgb(c.surface))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child("Общий чат")
                                    .child(button(
                                        "Очистить историю",
                                        move |_, cx| {
                                            clear.update(cx, |v, cx| {
                                                if !v.busy {
                                                    if let Some(s) = v.session.as_mut()
                                                        && let Err(e) = s.clear()
                                                    {
                                                        v.status = e.to_string();
                                                    }
                                                    cx.notify();
                                                }
                                            })
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id("chat-history")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .children(messages.into_iter().map(|m| {
                                        div()
                                            .p_2()
                                            .child(if m.role == "user" {
                                                "Вы"
                                            } else {
                                                "Ассистент"
                                            })
                                            .child(div().child(m.content))
                                    })),
                            )
                            .child(self.command.clone())
                            .child(button("Отправить", move |_, cx| {
                                send.update(cx, |v, cx| v.request(false, cx))
                            })),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(c.muted))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(button("Настройки размещения", crate::settings_ui::open))
                    .child(button("Показать тестовые тосты", |_, cx| schedule_demo(cx)))
                    .child(div().text_sm().child(
                        "Закрытие окна оставляет приложение в трее. Выход — через меню значка.",
                    )),
            )
    }
}
pub struct Toast {
    pub title: &'static str,
    pub text: &'static str,
    pub border: u32,
}

impl Render for Toast {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Palette::for_window(window);
        div()
            .size_full()
            .font_family("Segoe UI")
            .text_base()
            .text_color(rgb(colors.text))
            .border_2()
            .border_color(rgb(self.border))
            .bg(rgb(colors.surface))
            .p_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(self.title),
                    )
                    .child(
                        div()
                            .id("close-toast")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(rgb(colors.muted))
                            .hover(|style| style.bg(rgb(colors.background)))
                            .on_click(|_, window, _| window.remove_window())
                            .child("Закрыть ×"),
                    ),
            )
            .child(div().child(self.text))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(colors.muted))
                    .child("Останется до ручного закрытия"),
            )
    }
}

pub fn error_message(message: String) -> Div {
    div().p_6().font_family("Segoe UI").child(message)
}
