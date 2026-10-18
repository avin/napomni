use gpui::{App, Context, Div, IntoElement, Window, WindowAppearance, div, prelude::*, rgb};

use crate::schedule_demo;

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
        .px_4()
        .py_2()
        .bg(rgb(0x365edc))
        .text_color(rgb(0xffffff))
        .cursor_pointer()
        .hover(|style| style.bg(rgb(0x284ab9)))
        .on_click(move |_, window, cx| on_click(window, cx))
        .child(label)
}

pub struct MainWindow;

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = Palette::for_window(window);
        let panel = || {
            div()
                .flex()
                .flex_col()
                .gap_4()
                .p_6()
                .flex_1()
                .h_full()
                .border_1()
                .border_color(rgb(colors.border))
                .bg(rgb(colors.surface))
        };
        div().size_full().flex().flex_col().gap_5().p_6()
            .font_family("Segoe UI").text_base().text_color(rgb(colors.text)).bg(rgb(colors.background))
            .child(div().text_2xl().font_weight(gpui::FontWeight::SEMIBOLD).child("Напоминания"))
            .child(div().text_color(rgb(colors.muted)).child("Всё важное — в своё время"))
            .child(div().flex().gap_4().flex_1().min_h_0()
                .child(panel()
                    .child(div().text_lg().font_weight(gpui::FontWeight::SEMIBOLD).child("Список напоминаний"))
                    .child(div().text_color(rgb(colors.muted)).child("Здесь появятся ваши напоминания и расписания."))
                    .child(div().mt_4().child("Пока напоминаний нет")))
                .child(panel()
                    .child(div().text_lg().font_weight(gpui::FontWeight::SEMIBOLD).child("Общий чат"))
                    .child(div().text_color(rgb(colors.muted)).child("Здесь можно будет создавать и изменять напоминания обычными фразами."))
                    .child(div().mt_4().child("Например: «Напомни размяться через полчаса»"))))
            .child(div().flex().items_center().gap_4()
                .child(button("Настройки размещения", crate::settings_ui::open))
                .child(button("Показать тестовые тосты", |_, cx| schedule_demo(cx)))
                .child(div().text_sm().text_color(rgb(colors.muted)).child("Появятся через 5 секунд. Переключитесь в другое окно.")))
            .child(div().text_sm().text_color(rgb(colors.muted))
                .child("Закрытие окна оставляет приложение в трее. Выход — через меню значка."))
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
