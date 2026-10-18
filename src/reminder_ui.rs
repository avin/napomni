use crate::{native, settings_ui::DesktopSettings, ui::Toast};
use gpui::{
    App, AppContext, Bounds, Global, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, point, px, size,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use win_notifi_toast::{
    chat::ChatSession,
    reminders::{Application, JsonStore, Reminder, ReminderId, SystemClock, ToastSnapshot},
};

pub struct Reminders {
    pub app: Arc<Mutex<Application>>,
    windows: BTreeMap<ReminderId, WindowHandle<Toast>>,
    last_reminders: Vec<Reminder>,
    last_toasts: Vec<ToastSnapshot>,
}
impl Global for Reminders {}
pub fn initialize(cx: &mut App) -> anyhow::Result<()> {
    let app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(
            ChatSession::default_directory().join("reminders.json"),
        )),
    )?;
    cx.set_global(Reminders {
        app: Arc::new(Mutex::new(app)),
        windows: BTreeMap::new(),
        last_reminders: vec![],
        last_toasts: vec![],
    });
    Ok(())
}
pub fn close(id: ReminderId, cx: &mut App) {
    if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        app.close_toast(id);
    }
}
pub fn poll(cx: &mut App) -> anyhow::Result<bool> {
    let (reminders, toasts) = {
        let mut app = cx
            .global::<Reminders>()
            .app
            .lock()
            .map_err(|_| anyhow::anyhow!("Ядро напоминаний недоступно"))?;
        app.tick()?;
        (app.reminders().to_vec(), app.open_toasts().to_vec())
    };
    let changed = reminders != cx.global::<Reminders>().last_reminders
        || toasts != cx.global::<Reminders>().last_toasts;
    cx.global_mut::<Reminders>().last_reminders = reminders;
    cx.global_mut::<Reminders>().last_toasts = toasts.clone();
    cx.global_mut::<Reminders>()
        .windows
        .retain(|id, _| toasts.iter().any(|t| t.reminder_id == *id));
    let placement = cx.global::<DesktopSettings>().0.placement()?;
    let scale = placement.scale;
    for (index, snapshot) in toasts.into_iter().enumerate() {
        if cx
            .global::<Reminders>()
            .windows
            .contains_key(&snapshot.reminder_id)
        {
            continue;
        }
        let Some(rect) = placement.card(index, 180.) else {
            continue;
        };
        let id = snapshot.reminder_id;
        let handle = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(rect.x as f32 / scale), px(rect.y as f32 / scale)),
                    size(
                        px(rect.width as f32 / scale),
                        px(rect.height as f32 / scale),
                    ),
                ))),
                titlebar: None,
                kind: WindowKind::PopUp,
                focus: false,
                show: false,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Напоминание");
                cx.new(|_| Toast {
                    title: "Напоминание".into(),
                    text: snapshot.text,
                    border: snapshot.border,
                    reminder_id: Some(id),
                })
            },
        )?;
        let task = handle.update(cx, |_, window, cx| {
            native::place_toast(window, cx, rect.x, rect.y, rect.width, rect.height)
        })??;
        cx.spawn(async move |cx| {
            if let Err(error) = task.await {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
                let _ = cx.update(|cx| {
                    cx.global_mut::<Reminders>().windows.remove(&id);
                });
                eprintln!("Не удалось показать тост: {error:#}");
            }
        })
        .detach();
        cx.global_mut::<Reminders>().windows.insert(id, handle);
    }
    Ok(changed)
}
