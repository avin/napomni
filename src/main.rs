#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod input;
mod model_picker;
mod native;
mod reminder_ui;
mod settings_ui;
mod toast_stack;
mod ui;

use std::time::Duration;

use anyhow::Result;
use gpui::{
    App, AppContext, Application, Bounds, Global, TitlebarOptions, WindowBounds, WindowHandle,
    WindowOptions, px, size,
};
use tray_icon::{
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

struct Desktop {
    _tray: TrayIcon,
    main: WindowHandle<ui::MainWindow>,
    demo_pending: bool,
}

impl Global for Desktop {}

fn tray() -> Result<TrayIcon> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("open", "Открыть напоминания", true, None),
        &MenuItem::with_id("stack", "Открыть стопку тостов", true, None),
        &MenuItem::with_id("demo", "Показать тестовые тосты", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Выход", true, None),
    ])?;
    // A high contrast bell, rasterized in Rust so no external asset is needed.
    let mut rgba = vec![0; 32 * 32 * 4];
    for y in 0_i32..32 {
        for x in 0_i32..32 {
            let bell = ((8..24).contains(&x) && (11..23).contains(&y))
                || ((x - 16).pow(2) + (y - 11).pow(2) < 64 && y <= 11)
                || ((6..26).contains(&x) && (23..26).contains(&y))
                || ((x - 16).pow(2) + (y - 28).pow(2) < 7);
            if bell {
                let offset = ((y * 32 + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[105, 151, 255, 255]);
            }
        }
    }
    Ok(TrayIconBuilder::new()
        .with_tooltip("Напоминания")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon(Icon::from_rgba(rgba, 32, 32)?)
        .build()?)
}

fn open_main(cx: &mut App) {
    let main = cx.global::<Desktop>().main;
    let _ = main.update(cx, |_, window, _| {
        native::show(window);
        window.activate_window();
    });
}

fn schedule_demo(cx: &mut App) {
    if cx.global::<Desktop>().demo_pending {
        return;
    }
    cx.global_mut::<Desktop>().demo_pending = true;
    cx.spawn(async |cx| {
        cx.background_executor().timer(Duration::from_secs(5)).await;
        let _ = cx.update(|cx| {
            cx.global_mut::<Desktop>().demo_pending = false;
            if let Err(error) = show_demo(cx) {
                eprintln!("Не удалось показать тосты: {error:#}");
                let message = format!("Не удалось показать тестовые тосты: {error:#}");
                let _ = cx.open_window(WindowOptions::default(), |window, cx| {
                    native::square_corners(window);
                    cx.new(|_| ErrorView(message))
                });
            }
        });
    })
    .detach();
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
    Application::new().run(move |cx: &mut App| {
        input::bind_keys(cx);
        let result = (|| -> Result<()> {
            settings_ui::initialize(cx)?;
            reminder_ui::initialize(cx)?;
            let tray = tray()?;
            let main = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(960.), px(640.)),
                        cx,
                    ))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Напоминания".into()),
                        ..Default::default()
                    }),
                    window_min_size: Some(size(px(800.), px(560.))),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    window.on_window_should_close(cx, |window, _| {
                        native::hide(window);
                        false
                    });
                    cx.new(ui::MainWindow::new)
                },
            )?;
            cx.set_global(Desktop {
                _tray: tray,
                main,
                demo_pending: false,
            });
            cx.spawn(async |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(40))
                        .await;
                    if cx
                        .update(|cx| {
                            match reminder_ui::poll(cx) {
                                Ok(true) => {
                                    let main = cx.global::<Desktop>().main;
                                    let _ = main.update(cx, |_, _, cx| cx.notify());
                                }
                                Ok(false) => {}
                                Err(error) => {
                                    let main = cx.global::<Desktop>().main;
                                    let message = error.to_string();
                                    let _ = main.update(cx, |view, _, cx| {
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
                                    "demo" => schedule_demo(cx),
                                    "stack" => toast_stack::focus(cx),
                                    "quit" => {
                                        reminder_ui::shutdown(cx);
                                        cx.quit();
                                        return;
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .is_err()
                    {
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
