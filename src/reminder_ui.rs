use gpui::{App, Global};
use napomni::{
    chat::ChatSession,
    reminders::{Application, JsonStore, Reminder, SystemClock, ToastId, ToastSnapshot},
    sound::NativeSignal,
};
use std::sync::{Arc, Mutex};

pub struct Reminders {
    pub app: Arc<Mutex<Application>>,
    last_reminders: Vec<Reminder>,
    last_toasts: Vec<ToastSnapshot>,
    last_statuses: Vec<Option<String>>,
    signal: NativeSignal,
}
impl Global for Reminders {}
pub fn initialize(cx: &mut App) -> anyhow::Result<()> {
    let app = Application::open(
        Arc::new(SystemClock),
        Box::new(JsonStore::new(
            ChatSession::default_directory().join("reminders.json"),
        )),
    )?;
    crate::toast_stack::initialize(cx);
    cx.set_global(Reminders {
        app: Arc::new(Mutex::new(app)),
        last_reminders: vec![],
        last_toasts: vec![],
        last_statuses: vec![],
        signal: NativeSignal::default(),
    });
    Ok(())
}
pub fn close(id: ToastId, cx: &mut App) -> bool {
    let active = if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        if let Err(error) = app.close_toast(id) {
            eprintln!("{error:#}");
            return false;
        }
        app.signal_active()
    } else {
        return false;
    };
    if let Err(error) = cx.global_mut::<Reminders>().signal.reconcile(active) {
        eprintln!("{error:#}");
    }
    true
}
pub fn snooze(id: ToastId, cx: &mut App) -> bool {
    let minutes = cx
        .global::<crate::settings_ui::DesktopSettings>()
        .0
        .settings()
        .snooze_minutes;
    let active = if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        if let Err(error) = app.snooze_toast(id, minutes) {
            eprintln!("{error:#}");
            return false;
        }
        app.signal_active()
    } else {
        return false;
    };
    if let Err(error) = cx.global_mut::<Reminders>().signal.reconcile(active) {
        eprintln!("{error:#}");
    }
    true
}
pub fn shutdown(cx: &mut App) {
    if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        let ids = app.open_toasts().iter().map(|t| t.id).collect::<Vec<_>>();
        for id in ids {
            if let Err(error) = app.close_toast(id) {
                eprintln!("{error:#}");
            }
        }
    }
    if let Err(error) = cx.global_mut::<Reminders>().signal.reconcile(false) {
        eprintln!("{error:#}");
    }
}
pub fn poll(cx: &mut App) -> anyhow::Result<bool> {
    let (reminders, toasts, signal_active, statuses) = {
        let mut app = cx
            .global::<Reminders>()
            .app
            .lock()
            .map_err(|_| anyhow::anyhow!("Ядро напоминаний недоступно"))?;
        app.tick()?;
        (
            app.reminders().to_vec(),
            app.open_toasts().to_vec(),
            app.signal_active(),
            app.reminders()
                .iter()
                .map(|r| app.schedule_status(r.id))
                .collect::<Vec<_>>(),
        )
    };
    let sound_result = cx.global_mut::<Reminders>().signal.reconcile(signal_active);
    let changed = reminders != cx.global::<Reminders>().last_reminders
        || toasts != cx.global::<Reminders>().last_toasts
        || statuses != cx.global::<Reminders>().last_statuses;
    cx.global_mut::<Reminders>().last_statuses = statuses;
    cx.global_mut::<Reminders>().last_reminders = reminders;
    cx.global_mut::<Reminders>().last_toasts = toasts.clone();
    crate::toast_stack::sync(toasts, cx)?;
    sound_result?;
    Ok(changed)
}
