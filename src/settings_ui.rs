use gpui::{prelude::*, *};
use win_notifi_toast::placement::{Corner, SettingsService};

use crate::native::WindowsDesktop;

pub struct DesktopSettings(pub SettingsService<WindowsDesktop>);
impl Global for DesktopSettings {}

pub fn initialize(cx: &mut App) -> anyhow::Result<()> {
    let directory = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("Не задан каталог APPDATA"))?;
    cx.set_global(DesktopSettings(SettingsService::open(
        directory.join("WinNotifiToast").join("placement.json"),
        WindowsDesktop,
    )?));
    Ok(())
}

pub fn open(_: &mut Window, cx: &mut App) {
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(620.), px(500.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("Настройки размещения".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            crate::native::square_corners(window);
            cx.new(|_| SettingsView {
                message: String::new(),
            })
        },
    );
}

struct SettingsView {
    message: String,
}
impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = matches!(
            window.appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        let settings = cx.global::<DesktopSettings>().0.settings().clone();
        let monitors = cx.global::<DesktopSettings>().0.monitors();
        let mut content = div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .font_family("Segoe UI")
            .text_color(rgb(if dark { 0xf3f4f6 } else { 0x20242c }))
            .bg(rgb(if dark { 0x202329 } else { 0xffffff }))
            .child(div().text_xl().child("Размещение тостов"))
            .child(div().child("Монитор (нажмите для выбора)"));
        let monitors = match monitors {
            Ok(monitors) => monitors,
            Err(error) => {
                self.message = error.to_string();
                Vec::new()
            }
        };
        for (index, monitor) in monitors.into_iter().enumerate() {
            let selected = settings
                .monitor
                .as_ref()
                .map_or(monitor.primary, |id| id == &monitor.id);
            let label = format!(
                "{} {}{}",
                if selected { "●" } else { "○" },
                monitor.id,
                if monitor.primary {
                    " (основной)"
                } else {
                    ""
                }
            );
            content = content.child(
                div()
                    .id(("monitor", index))
                    .cursor_pointer()
                    .p_2()
                    .border_1()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut next = cx.global::<DesktopSettings>().0.settings().clone();
                        next.monitor = Some(monitor.id.clone());
                        this.message = match cx.global_mut::<DesktopSettings>().0.save(next) {
                            Ok(()) => "Настройки сохранены".into(),
                            Err(error) => error.to_string(),
                        };
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let mut corners = div().flex().gap_2();
        for (index, corner) in Corner::ALL.into_iter().enumerate() {
            corners = corners.child(
                div()
                    .id(("corner", index))
                    .cursor_pointer()
                    .p_2()
                    .border_1()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut next = cx.global::<DesktopSettings>().0.settings().clone();
                        next.corner = corner;
                        this.message = match cx.global_mut::<DesktopSettings>().0.save(next) {
                            Ok(()) => "Настройки сохранены".into(),
                            Err(error) => error.to_string(),
                        };
                        cx.notify();
                    }))
                    .child(format!(
                        "{} {}",
                        if settings.corner == corner {
                            "●"
                        } else {
                            "○"
                        },
                        corner.label()
                    )),
            );
        }
        content
            .child(corners)
            .child(
                div()
                    .id("autostart")
                    .p_2()
                    .border_1()
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let mut next = cx.global::<DesktopSettings>().0.settings().clone();
                        next.autostart = !next.autostart;
                        this.message = match cx.global_mut::<DesktopSettings>().0.save(next) {
                            Ok(()) => "Настройки сохранены".into(),
                            Err(error) => error.to_string(),
                        };
                        cx.notify();
                    }))
                    .child(format!(
                        "{} Автозапуск при входе в Windows",
                        if settings.autostart { "☑" } else { "☐" }
                    )),
            )
            .child("Изменения применяются к следующим тостам. Закрытые тосты не восстанавливаются.")
            .child(self.message.clone())
    }
}
