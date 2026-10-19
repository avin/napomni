#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

extern crate gpui_kit as gpui;
mod model_picker;
mod native;
mod reminder_ui;
mod settings_ui;
mod toast_stack;
mod ui;

use std::time::Duration;

use anyhow::Result;
use gpui::{
    App, AppContext, Bounds, Entity, Focusable, Global, TitlebarOptions, WindowBounds,
    WindowHandle, WindowOptions, px, size,
};
use tray_icon::{
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

struct Desktop {
    _tray: TrayIcon,
    main: WindowHandle<gpui::component::Root>,
    view: Entity<ui::MainWindow>,
}

impl Global for Desktop {}

fn tray() -> Result<TrayIcon> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("open", "Открыть напоминания", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Выход", true, None),
    ])?;
    Ok(TrayIconBuilder::new()
        .with_tooltip("Напоминания")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(Icon::from_resource(1, Some((32, 32)))?)
        .build()?)
}

fn open_main(cx: &mut App) {
    let main = cx.global::<Desktop>().main;
    let view = cx.global::<Desktop>().view.clone();
    let _ = main.update(cx, |_, window, cx| {
        native::show(window);
        window.activate_window();
        view.read(cx)
            .command
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    });
}

fn schedule_demo(cx: &mut App) {
    if let Err(error) = show_demo(cx) {
        eprintln!("Не удалось показать тосты: {error:#}");
        let message = format!("Не удалось показать тестовые тосты: {error:#}");
        let _ = cx.open_window(WindowOptions::default(), |window, cx| {
            native::square_corners(window);
            cx.new(|_| ErrorView(message))
        });
    }
}

fn show_demo(cx: &mut App) -> Result<()> {
    toast_stack::show_demo(cx)
}

struct ErrorView(String);
impl gpui::Render for ErrorView {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        ui::error_message(self.0.clone())
    }
}

fn main() {
    let demo = std::env::args().any(|arg| arg == "--test-toasts");
    let show_main = std::env::args().any(|arg| arg == "--open");
    gpui::application()
        .with_assets(gpui::assets::AllAssets)
        .run(move |cx: &mut App| {
            gpui::init(cx);
            gpui::component::Theme::sync_system_appearance(None, cx);
            let result = (|| -> Result<()> {
                settings_ui::initialize(cx)?;
                reminder_ui::initialize(cx)?;
                let tray = tray()?;
                let mut main_view = None;
                let main = cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                            None,
                            size(px(1200.), px(800.)),
                            cx,
                        ))),
                        titlebar: Some(TitlebarOptions {
                            title: Some("Напоминания".into()),
                            ..Default::default()
                        }),
                        window_min_size: Some(size(px(960.), px(640.))),
                        show: false,
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        window
                            .observe_window_appearance(|window, cx| {
                                gpui::component::Theme::sync_system_appearance(Some(window), cx);
                            })
                            .detach();
                        window.on_window_should_close(cx, |window, _| {
                            native::hide(window);
                            false
                        });
                        let view = cx.new(|cx| ui::MainWindow::new(window, cx));
                        main_view = Some(view.clone());
                        cx.new(|cx| gpui::component::Root::new(view, window, cx))
                    },
                )?;
                cx.set_global(Desktop {
                    _tray: tray,
                    main,
                    view: main_view.unwrap(),
                });
                cx.spawn(async |cx| {
                    loop {
                        cx.background_executor()
                            .timer(Duration::from_millis(40))
                            .await;
                        if cx.update(|cx| {
                            match reminder_ui::poll(cx) {
                                Ok(true) => {
                                    let view = cx.global::<Desktop>().view.clone();
                                    view.update(cx, |_, cx| cx.notify());
                                }
                                Ok(false) => {}
                                Err(error) => {
                                    let view = cx.global::<Desktop>().view.clone();
                                    let message = error.to_string();
                                    view.update(cx, |view, cx| {
                                        view.status = message;
                                        cx.notify();
                                    });
                                }
                            }
                            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                                if matches!(
                                    event,
                                    TrayIconEvent::DoubleClick {
                                        button: MouseButton::Left,
                                        ..
                                    }
                                ) {
                                    open_main(cx);
                                }
                            }
                            while let Ok(event) = MenuEvent::receiver().try_recv() {
                                match event.id.as_ref() {
                                    "open" => open_main(cx),
                                    "quit" => {
                                        reminder_ui::shutdown(cx);
                                        cx.quit();
                                        return true;
                                    }
                                    _ => {}
                                }
                            }
                            false
                        }) {
                            break;
                        }
                    }
                })
                .detach();
                if demo {
                    schedule_demo(cx);
                }
                if show_main {
                    open_main(cx);
                }
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Не удалось запустить приложение: {error:#}");
                cx.quit();
            }
        });
}
