use crate::ui::MainWindow;
use gpui::assets::IconName;
use gpui::{
    component::{
        ActiveTheme, Icon, WindowExt,
        button::Button,
        input::{Input, InputState},
    },
    prelude::*,
    *,
};
use std::{sync::mpsc, time::Duration};
use win_notifi_toast::{
    catalog::{Catalog, ToolSupport},
    chat::Connection,
};

pub fn open(connection: Connection, parent: Entity<MainWindow>, window: &mut Window, cx: &mut App) {
    let picker = cx.new(|cx| ModelPicker::new(connection, parent, window, cx));
    let initial_focus = std::cell::Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if initial_focus.replace(false) {
            picker.read(cx).search.focus_handle(cx).focus(window, cx);
        }
        dialog.title("Каталог моделей").width(px(720.)).child(
            div()
                .h((window.viewport_size().height - px(180.)).min(px(480.)))
                .child(picker.clone()),
        )
    });
}

struct ModelPicker {
    connection: Connection,
    parent: Entity<MainWindow>,
    catalog: Catalog,
    search: Entity<InputState>,
    status: String,
    loading: bool,
    failed: bool,
}
impl ModelPicker {
    fn new(
        connection: Connection,
        parent: Entity<MainWindow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Поиск по названию или ID модели"));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let (sender, receiver) = mpsc::channel();
        let settings = connection.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Не удалось запустить загрузку каталога".to_owned())
                .and_then(|rt| {
                    rt.block_on(Catalog::load(&settings))
                        .map_err(|error| error.to_string())
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(Duration::from_millis(40)).await;
            match receiver.try_recv() {
                Ok(result) => {
                    let _ = view.update(cx, |view, cx| {
                        view.loading = false;
                        match result {
                            Ok(catalog) => {
                                view.status.clear();
                                view.catalog = catalog;
                            }
                            Err(error) => { view.failed = true; view.status = error; }
                        }
                        cx.notify();
                    });
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = view.update(cx, |view, cx| {
                        view.loading = false;
                        view.failed = true;
                        view.status = "Не удалось получить каталог. Введите ID вручную в настройках подключения.".into();
                        cx.notify();
                    });
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }).detach();
        Self {
            connection,
            parent,
            catalog: Catalog::default(),
            search,
            status: String::new(),
            loading: true,
            failed: false,
        }
    }
    fn select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = self.parent.update(cx, |parent, cx| {
            parent.select_catalog_model(&self.catalog, id, &self.connection, window, cx)
        });
        match outcome {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.failed = true;
                self.status = error.to_string();
                cx.notify();
            }
        }
    }
}
impl Render for ModelPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let foreground = theme.foreground;
        let muted = theme.muted;
        let muted_text = theme.muted_foreground;
        let border = theme.border;
        let danger = theme.danger;
        let results = self
            .catalog
            .search(&self.search.read(cx).value())
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let count = results.len();
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI")
            .text_base()
            .bg(background)
            .text_color(foreground)
            .child(
                div()
                    .p_3()
                    .border_b_1()
                    .border_color(border)
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.search)
                                .prefix(Icon::new(IconName::Search))
                                .cleanable(true),
                        ),
                    )
                    .when(!self.loading && !self.failed, |e| {
                        e.child(
                            div()
                                .text_sm()
                                .text_color(muted_text)
                                .child(format!("{count}")),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_3()
                    .when(count == 0, |view| {
                        view.child(
                            div()
                                .size_full()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .rounded_lg()
                                .bg(muted)
                                .p_3()
                                .child(div().font_weight(FontWeight::SEMIBOLD).child(
                                    if self.loading {
                                        "Загружаем модели"
                                    } else if self.failed {
                                        "Каталог недоступен"
                                    } else {
                                        "Ничего не найдено"
                                    },
                                )),
                        )
                    })
                    .when(count > 0, |view| {
                        view.child(
                            uniform_list(
                                "catalog-results",
                                count,
                                cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                                    range
                                        .map(|index| {
                                            let model = results[index].clone();
                                            let id = model.id.clone();
                                            let supported =
                                                model.tool_support == ToolSupport::Supported;
                                            div().h(px(80.)).pb_1().child(
                                                div()
                                                    .size_full()
                                                    .p_2()
                                                    .rounded_lg()
                                                    .border_1()
                                                    .border_color(border)
                                                    .flex()
                                                    .items_center()
                                                    .gap_3()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .flex()
                                                            .flex_col()
                                                            .gap_1()
                                                            .child(
                                                                div()
                                                                    .truncate()
                                                                    .font_weight(
                                                                        FontWeight::SEMIBOLD,
                                                                    )
                                                                    .child(model.name),
                                                            )
                                                            .child(
                                                                div()
                                                                    .truncate()
                                                                    .text_sm()
                                                                    .text_color(muted_text)
                                                                    .child(model.id),
                                                            ),
                                                    )
                                                    .child(
                                                        Button::new(("select-model", index))
                                                            .icon(IconName::Check)
                                                            .label("Выбрать")
                                                            .tooltip(if supported {
                                                                "Инструменты поддерживаются"
                                                            } else {
                                                                "Нужна проверка инструментов"
                                                            })
                                                            .on_click(cx.listener(
                                                                move |view, _, window, cx| {
                                                                    view.select(&id, window, cx)
                                                                },
                                                            )),
                                                    ),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                }),
                            )
                            .size_full(),
                        )
                    }),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(border)
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().flex_1().min_w_0().when(self.failed, |e| {
                        e.child(
                            div()
                                .text_sm()
                                .text_color(danger)
                                .child(self.status.clone()),
                        )
                    }))
                    .child(
                        Button::new("close-model-picker")
                            .icon(IconName::X)
                            .label("Закрыть")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}
