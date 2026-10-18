use crate::{input::TextInput, ui::MainWindow};
use gpui::{prelude::*, *};
use std::{sync::mpsc, time::Duration};
use win_notifi_toast::{
    catalog::{Catalog, ToolSupport},
    chat::Connection,
};

pub fn open(connection: Connection, parent: Entity<MainWindow>, cx: &mut App) {
    let result = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(720.), px(640.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Каталог моделей".into()),
                ..Default::default()
            }),
            window_min_size: Some(size(px(480.), px(400.))),
            ..Default::default()
        },
        move |window, cx| {
            crate::native::square_corners(window);
            cx.new(|cx| ModelPicker::new(connection, parent, cx))
        },
    );
    if let Err(e) = result {
        eprintln!("Не удалось открыть каталог моделей: {e}");
    }
}
struct ModelPicker {
    connection: Connection,
    parent: Entity<MainWindow>,
    catalog: Catalog,
    search: Entity<TextInput>,
    status: String,
}
impl ModelPicker {
    fn new(connection: Connection, parent: Entity<MainWindow>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| TextInput::new("", "Часть имени или ID", cx));
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        let (sender, receiver) = mpsc::channel();
        let settings = connection.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Runtime::new()
                .map_err(|_| "Не удалось запустить загрузку каталога".to_owned())
                .and_then(|rt| {
                    rt.block_on(Catalog::load(&settings))
                        .map_err(|e| e.to_string())
                });
            let _ = sender.send(result);
        });
        cx.spawn(async move|view,cx|loop {
   cx.background_executor().timer(Duration::from_millis(40)).await;
   match receiver.try_recv() {
    Ok(result)=>{let _=view.update(cx,|view,cx|{match result {Ok(catalog)=>{view.status="Модели без сведений об инструментах потребуют проверки подключения".into();view.catalog=catalog;},Err(e)=>view.status=e}cx.notify();});break;},
    Err(mpsc::TryRecvError::Disconnected)=>{let _=view.update(cx,|view,cx|{view.status="Не удалось получить каталог. ID можно ввести вручную в главном окне.".into();cx.notify();});break;},
    Err(mpsc::TryRecvError::Empty)=>{}
   }
  }).detach();
        Self {
            connection,
            parent,
            catalog: Catalog::default(),
            search,
            status: "Загружаем каталог…".into(),
        }
    }
}
impl Render for ModelPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let surface = if dark { 0x202329 } else { 0xffffff };
        let text = if dark { 0xf3f4f6 } else { 0x20242c };
        let muted = if dark { 0xb1b8c4 } else { 0x5e6878 };
        let border = if dark { 0x393e48 } else { 0xdce0e7 };
        let results = self
            .catalog
            .search(self.search.read(cx).value())
            .into_iter()
            .cloned()
            .collect::<Vec<_>>();
        let count = results.len();
        div().size_full().flex().flex_col().gap_3().p_5().font_family("Segoe UI").text_base().bg(rgb(surface)).text_color(rgb(text))
   .child(div().text_xl().child("Выберите модель"))
   .child(div().text_sm().text_color(rgb(muted)).child("Поиск по имени и ID. Модели с явно отсутствующей поддержкой инструментов скрыты."))
   .child(self.search.clone())
   .child(div().text_sm().child(format!("Найдено: {count}")))
   .child(uniform_list("catalog-results",count,cx.processor(move|_view,range: std::ops::Range<usize>,_,cx| {
    range.map(|index|{let model=results[index].clone();let id=model.id.clone();
     div().id(index).h(px(84.)).p_2().border_b_1().border_color(rgb(border)).cursor_pointer().hover(|s|s.bg(rgb(if dark {0x30343d}else{0xf1f4fa})))
      .child(div().overflow_hidden().child(model.name))
      .child(div().text_sm().overflow_hidden().child(model.id))
      .child(div().text_sm().text_color(rgb(muted)).child(match model.tool_support {ToolSupport::Supported=>"Инструменты заявлены провайдером; проверим перед запросом",_=>"Поддержка инструментов неизвестна; нужна проверка"}))
      .on_click(cx.listener(move|view,_,window,cx|{
       let outcome=view.parent.update(cx,|parent,cx|parent.select_catalog_model(&view.catalog,&id,&view.connection,cx));
       match outcome {Ok(())=>window.remove_window(),Err(e)=>{view.status=e.to_string();cx.notify();}}
      }))
    }).collect::<Vec<_>>()
   })).flex_1().min_h_0())
   .child(div().text_sm().text_color(rgb(muted)).child(self.status.clone()))
   .child(div().id("close-model-picker").p_2().border_1().border_color(rgb(border)).cursor_pointer().child("Закрыть — ID можно ввести вручную").on_click(|_,window,_|window.remove_window()))
    }
}
