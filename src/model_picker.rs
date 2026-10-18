use crate::ui::MainWindow;
use gpui::{
    component::{
        ActiveTheme, Icon, IconName,
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

pub fn open(connection: Connection, parent: Entity<MainWindow>, cx: &mut App) {
    let result = gpui::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(800.), px(700.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Каталог моделей".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(580.), px(440.))),
            ..Default::default()
        },
        cx,
        move |window, cx| {
            crate::native::square_corners(window);
            cx.new(|cx| ModelPicker::new(connection, parent, window, cx))
        },
    );
    if let Err(error) = result {
        eprintln!("Не удалось открыть каталог моделей: {error}");
    }
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
                                view.status = "Поддержку инструментов проверим при подключении.".into();
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
            status: "Загружаем каталог…".into(),
            loading: true,
            failed: false,
        }
    }
    fn select(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let outcome = self.parent.update(cx, |parent, cx| {
            parent.select_catalog_model(&self.catalog, id, &self.connection, window, cx)
        });
        match outcome {
            Ok(()) => window.remove_window(),
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
        let primary = theme.primary;
        let danger = theme.danger;
        let results = self
            .catalog
            .search(&self.search.read(cx).value())
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let count = results.len();
        div().size_full().flex().flex_col().font_family("Segoe UI").text_base().bg(background).text_color(foreground)
            .child(div().p_6().pb_5().border_b_1().border_color(border).flex().flex_col().gap_4()
                .child(div().child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child("Модель для ваших напоминаний"))
                    .child(div().mt_2().text_sm().text_color(muted_text).child("Выберите модель, которая умеет работать с инструментами.")))
                .child(Input::new(&self.search).prefix(Icon::new(IconName::Search)).cleanable(true)))
            .child(div().px_6().py_3().text_sm().text_color(muted_text).child(if self.loading { "Каталог обновляется…".to_owned() } else { format!("Доступно моделей: {count}") }))
            .child(div().flex_1().min_h_0().px_6().when(count == 0, |view| view.child(
                div().size_full().flex().flex_col().items_center().justify_center().gap_3().rounded_lg().bg(muted).p_6()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(if self.loading { "Загружаем модели" } else if self.failed { "Каталог недоступен" } else { "Ничего не найдено" }))
                    .child(div().text_sm().text_color(muted_text).child(if self.loading { "Это займёт несколько секунд." } else if self.failed { "ID модели можно указать вручную в настройках подключения." } else { "Попробуйте другую часть названия или ID." }))
            )).when(count > 0, |view| view.child(
                uniform_list("catalog-results", count, cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                    range.map(|index| {
                        let model = results[index].clone();
                        let id = model.id.clone();
                        let supported = model.tool_support == ToolSupport::Supported;
                        div().h(px(116.)).pb_2().child(
                            div().size_full().p_4().rounded_lg().border_1().border_color(border).flex().items_center().gap_4()
                                .child(div().flex_1().min_w_0().flex().flex_col().gap_1()
                                    .child(div().truncate().font_weight(FontWeight::SEMIBOLD).child(model.name))
                                    .child(div().truncate().text_sm().text_color(muted_text).child(model.id))
                                    .child(div().text_xs().text_color(if supported { primary } else { muted_text })
                                        .child(if supported { "Инструменты поддерживаются" } else { "Поддержку инструментов нужно проверить" })))
                                .child(Button::new(("select-model", index)).label("Выбрать").on_click(cx.listener(move |view, _, window, cx| view.select(&id, window, cx))))
                        )
                    }).collect::<Vec<_>>()
                })).size_full()
            )))
            .child(div().px_6().py_4().mt_3().border_t_1().border_color(border).flex().items_center().gap_4()
                .child(div().flex_1().min_w_0().text_sm().text_color(if self.failed { danger } else { muted_text }).child(self.status.clone()))
                .child(Button::new("close-model-picker").label("Закрыть").on_click(|_, window, _| window.remove_window())))
    }
}
