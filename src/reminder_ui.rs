use gpui::{App, Global};
use std::sync::{Arc, Mutex};
use win_notifi_toast::{
    chat::ChatSession,
    reminders::{Application, JsonStore, Reminder, ReminderId, SystemClock, ToastSnapshot},
    sound::NativeSignal,
};

pub struct Reminders {
    pub app: Arc<Mutex<Application>>,
    last_reminders: Vec<Reminder>,
    last_toasts: Vec<ToastSnapshot>,
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
        signal: NativeSignal::default(),
    });
    Ok(())
}
pub fn close(id: ReminderId, cx: &mut App) {
    let active = if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        app.close_toast(id);
        app.signal_active()
    } else {
        false
    };
    if let Err(error) = cx.global_mut::<Reminders>().signal.reconcile(active) {
        eprintln!("{error:#}");
    }
}
pub fn shutdown(cx: &mut App) {
    if let Ok(mut app) = cx.global::<Reminders>().app.lock() {
        let ids = app
            .open_toasts()
            .iter()
            .map(|t| t.reminder_id)
            .collect::<Vec<_>>();
        for id in ids {
            app.close_toast(id);
        }
    }
    if let Err(error) = cx.global_mut::<Reminders>().signal.reconcile(false) {
        eprintln!("{error:#}");
    }
}
pub fn poll(cx: &mut App) -> anyhow::Result<bool> {
    let (reminders, toasts, signal_active) = {
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
        )
    };
    let sound_result = cx.global_mut::<Reminders>().signal.reconcile(signal_active);
    let changed = reminders != cx.global::<Reminders>().last_reminders
        || toasts != cx.global::<Reminders>().last_toasts;
    cx.global_mut::<Reminders>().last_reminders = reminders;
    cx.global_mut::<Reminders>().last_toasts = toasts.clone();
    crate::toast_stack::sync(toasts, cx)?;
    sound_result?;
    Ok(changed)
}
