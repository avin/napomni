use gpui::assets::IconName;
use gpui::{
    component::{
        ActiveTheme, Selectable, WindowExt,
        button::{Button, ButtonVariants},
        radio::Radio,
    },
    prelude::*,
    *,
};
use napomni::placement::{Corner, Settings, SettingsService};

use crate::native::WindowsDesktop;

pub struct DesktopSettings(pub SettingsService<WindowsDesktop>);
impl Global for DesktopSettings {}

pub fn initialize(cx: &mut App) -> anyhow::Result<()> {
    let directory = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Не задан каталог APPDATA"))?;
    cx.set_global(DesktopSettings(SettingsService::open(
        directory.join("napomni").join("placement.json"),
        WindowsDesktop,
    )?));
    Ok(())
}

pub fn open(window: &mut Window, cx: &mut App) {
    let settings = cx.new(|_| SettingsView {
        message: String::new(),
        monitor_error: None,
        scroll: ScrollHandle::new(),
    });
    window.open_dialog(cx, move |dialog, window, _| {
        dialog.title("Размещение тостов").width(px(520.)).child(
            div()
                .h((window.viewport_size().height - px(180.)).min(px(340.)))
                .child(settings.clone()),
        )
    });
}

struct SettingsView {
    message: String,
    monitor_error: Option<String>,
    scroll: ScrollHandle,
}
impl SettingsView {
    fn save(&mut self, next: Settings, cx: &mut Context<Self>) {
        match cx.global_mut::<DesktopSettings>().0.save(next) {
            Ok(()) => {
                self.message.clear();
            }
            Err(error) => {
                self.message = error.to_string();
            }
        }
        cx.notify();
    }
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let foreground = theme.foreground;
        let muted = theme.muted;
        let muted_text = theme.muted_foreground;
        let border = theme.border;
        let primary = theme.primary;
        let settings = cx.global::<DesktopSettings>().0.settings().clone();
        let monitors = match cx.global::<DesktopSettings>().0.monitors() {
            Ok(monitors) => {
                self.monitor_error = None;
                monitors
            }
            Err(error) => {
                let message = error.to_string();
                if self.monitor_error.as_ref() != Some(&message) {
                    self.message = message.clone();
                    self.monitor_error = Some(message);
                }
                Vec::new()
            }
        };
        crate::ui::notify_pending(
            &mut self.message,
            gpui::component::notification::NotificationType::Error,
            window,
            cx,
        );
        let mut monitor_list = div().flex().flex_col().gap_2();
        for (index, monitor) in monitors.into_iter().enumerate() {
            let selected = settings
                .monitor
                .as_ref()
                .map_or(monitor.primary, |id| id == &monitor.id);
            monitor_list = monitor_list.child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded_lg()
                    .border_1()
                    .border_color(if selected { primary } else { border })
                    .bg(if selected { muted } else { background })
                    .child(
                        Radio::new(("monitor", index))
                            .checked(selected)
                            .label(format!(
                                "Экран {}{}",
                                index + 1,
                                if monitor.primary {
                                    " · основной"
                                } else {
                                    ""
                                }
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let mut next = cx.global::<DesktopSettings>().0.settings().clone();
                                next.monitor = Some(monitor.id.clone());
                                this.save(next, cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(div().text_sm().text_color(muted_text).child(format!(
                        "{} × {}",
                        monitor.area.right - monitor.area.left,
                        monitor.area.bottom - monitor.area.top
                    ))),
            );
        }
        let mut corner_rows = div().flex().flex_col().gap_2();
        for (row, corners) in [
            [Corner::TopLeft, Corner::TopRight],
            [Corner::BottomLeft, Corner::BottomRight],
        ]
        .into_iter()
        .enumerate()
        {
            let mut line = div().flex().gap_2();
            for (column, corner) in corners.into_iter().enumerate() {
                line = line.child(
                    Button::new(("corner", row * 2 + column))
                        .icon(match corner {
                            Corner::TopLeft => IconName::ArrowUpLeft,
                            Corner::TopRight => IconName::ArrowUpRight,
                            Corner::BottomLeft => IconName::ArrowDownLeft,
                            Corner::BottomRight => IconName::ArrowDownRight,
                        })
                        .flex_1()
                        .label(corner.label())
                        .toggled(settings.corner == corner)
                        .selected(settings.corner == corner)
                        .when(settings.corner == corner, |button| button.primary())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut next = cx.global::<DesktopSettings>().0.settings().clone();
                            next.corner = corner;
                            this.save(next, cx);
                        })),
                );
            }
            corner_rows = corner_rows.child(line);
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI")
            .text_base()
            .bg(background)
            .text_color(foreground)
            .child(crate::ui::scroll_panel(
                div()
                    .id("settings-scroll")
                    .track_scroll(&self.scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Монитор"))
                            .child(monitor_list),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Угол экрана"))
                            .child(div().rounded_lg().bg(muted).p_3().child(corner_rows)),
                    ),
                &self.scroll,
            ))
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(border)
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(div().flex_1())
                    .child(
                        Button::new("close-settings")
                            .icon(IconName::Check)
                            .primary()
                            .label("Готово")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
    }
}
