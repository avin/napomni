use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::{
    external_commands::execute,
    reminders::{
        Application, Clock, CommandContext, JsonStore, Persisted, ReminderStore, TimeSample,
    },
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new() -> Self {
        Self(Mutex::new(TimeSample {
            utc: "2026-10-19T08:00:00Z".parse().unwrap(),
            awake_millis: 0,
        }))
    }
    fn advance(&self, seconds: i64) {
        let mut sample = self.0.lock().unwrap();
        sample.utc += Duration::seconds(seconds);
        sample.awake_millis += seconds as u64 * 1000;
    }
}
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, at: DateTime<Utc>) -> NaiveDateTime {
        at.naive_utc()
    }
    fn resolve_local(&self, at: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![at.and_utc()]
    }
}

#[test]
fn immediate_snooze_survives_restart_at_its_original_deadline_and_can_be_repeated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reminders.json");
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    execute(
        &mut app,
        "notify",
        &json!({"text":"Unicode \u{412}\u{43e}\u{434}\u{430} \"quote\"\nnext"}),
    )
    .unwrap();
    let snapshot = app.open_toasts()[0].clone();
    app.snooze_toast(snapshot.id, 5).unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
    assert!(app.reminders().is_empty());
    assert!(!app.can_undo());
    clock.advance(120);
    drop(app);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(179);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    let shown = &app.open_toasts()[0];
    assert_eq!(shown.id, snapshot.id);
    assert_eq!(shown.text, snapshot.text);
    assert_eq!(shown.border, snapshot.border);
    assert_eq!(shown.sound, snapshot.sound);
    assert_eq!(shown.reminder_id, None);
    assert!(app.signal_active());
    app.snooze_toast(snapshot.id, 2).unwrap();
    clock.advance(120);
    app.tick().unwrap();
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].id, snapshot.id);
    app.close_toast(snapshot.id).unwrap();
    clock.advance(86_400);
    app.tick().unwrap();
    drop(app);
    let app = Application::open(clock, Box::new(JsonStore::new(path))).unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
    assert!(app.reminders().is_empty());
}

#[test]
fn undo_preserves_identical_pending_notifications_and_regular_schedules_do_not_replace_them() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("reminders.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "chat", None);
    let reminder = app.create(serde_json::from_value(json!({"text":"Scheduled","schedule":{"kind":"after_seconds","seconds":60},"sound":false})).unwrap(), &context).unwrap();
    app.delete(reminder.id).unwrap();
    execute(&mut app, "notify", &json!({"text":"Same text"})).unwrap();
    let first = app.open_toasts()[0].clone();
    execute(&mut app, "notify", &json!({"text":"Same text"})).unwrap();
    let second = app.open_toasts()[0].clone();
    app.snooze_toast(first.id, 5).unwrap();
    assert!(app.signal_active());
    assert_eq!(app.open_toasts(), std::slice::from_ref(&second));
    app.snooze_toast(second.id, 3).unwrap();
    assert!(!app.signal_active());
    let restored = app.undo().unwrap().unwrap();
    assert_eq!(
        restored.id, reminder.id,
        "snoozing must preserve reminder undo history"
    );
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
    assert!(!app.signal_active());
    clock.advance(120);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 2);
    assert_eq!(app.open_toasts()[0].id, second.id);
    assert!(app.signal_active());
    app.close_toast(second.id).unwrap();
    assert!(!app.signal_active());
    clock.advance(120);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 2);
    assert_eq!(app.open_toasts()[0].id, first.id);
    assert_eq!(app.open_toasts()[0].text, first.text);
    assert!(app.signal_active());
    app.close_toast(first.id).unwrap();
    assert!(!app.signal_active());
}

struct FailableStore {
    store: JsonStore,
    fail: Arc<AtomicBool>,
}
impl ReminderStore for FailableStore {
    fn load(&self) -> anyhow::Result<Persisted> {
        self.store.load()
    }
    fn save(&self, state: &Persisted) -> anyhow::Result<()> {
        anyhow::ensure!(!self.fail.load(Ordering::SeqCst), "storage unavailable");
        self.store.save(state)
    }
}

#[test]
fn failed_immediate_snooze_preserves_the_card_and_retry_sets_the_actual_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reminders.json");
    let clock = Arc::new(TestClock::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut app = Application::open(
        clock.clone(),
        Box::new(FailableStore {
            store: JsonStore::new(&path),
            fail: fail.clone(),
        }),
    )
    .unwrap();
    execute(&mut app, "notify", &json!({"text":"Keep me"})).unwrap();
    let snapshot = app.open_toasts()[0].clone();
    fail.store(true, Ordering::SeqCst);
    assert!(app.snooze_toast(snapshot.id, 5).is_err());
    assert_eq!(app.open_toasts(), std::slice::from_ref(&snapshot));
    assert!(app.signal_active());
    assert!(app.reminders().is_empty());
    clock.advance(120);
    fail.store(false, Ordering::SeqCst);
    app.snooze_toast(snapshot.id, 5).unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
    drop(app);
    clock.advance(299);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].id, snapshot.id);
    assert!(app.signal_active());
}

#[test]
fn failed_immediate_redisplay_can_retry_or_restart_without_loss_or_duplicate() {
    for restart in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reminders.json");
        let clock = Arc::new(TestClock::new());
        let fail = Arc::new(AtomicBool::new(false));
        let store = || {
            Box::new(FailableStore {
                store: JsonStore::new(&path),
                fail: fail.clone(),
            }) as Box<dyn ReminderStore>
        };
        let mut app = Application::open(clock.clone(), store()).unwrap();
        execute(&mut app, "notify", &json!({"text":"Durable pending"})).unwrap();
        let snapshot = app.open_toasts()[0].clone();
        app.snooze_toast(snapshot.id, 5).unwrap();
        clock.advance(300);
        fail.store(true, Ordering::SeqCst);
        for _ in 0..2 {
            assert!(app.tick().is_err());
            assert!(app.open_toasts().is_empty());
            assert!(!app.signal_active());
        }
        if restart {
            drop(app);
            assert!(Application::open(clock.clone(), store()).is_err());
            fail.store(false, Ordering::SeqCst);
            app = Application::open(clock.clone(), store()).unwrap();
        } else {
            fail.store(false, Ordering::SeqCst);
            app.tick().unwrap();
        }
        assert_eq!(app.open_toasts().len(), 1);
        let shown = &app.open_toasts()[0];
        assert_eq!(shown.id, snapshot.id);
        assert_eq!(shown.text, snapshot.text);
        assert_eq!(shown.border, snapshot.border);
        assert_eq!(shown.sound, snapshot.sound);
        assert!(app.signal_active());
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        drop(app);
        let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
        assert!(app.open_toasts().is_empty());
        assert!(!app.signal_active());
    }
}

#[test]
fn immediate_snooze_is_restored_once_when_starting_at_or_after_the_deadline() {
    for elapsed in [300, 900] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("reminders.json");
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        execute(&mut app, "notify", &json!({"text":"Restore once"})).unwrap();
        let snapshot = app.open_toasts()[0].clone();
        app.snooze_toast(snapshot.id, 5).unwrap();
        drop(app);
        clock.advance(elapsed);
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        assert_eq!(app.open_toasts()[0].id, snapshot.id);
        assert!(app.signal_active());
        app.tick().unwrap();
        app.resume().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        drop(app);
        let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
        assert!(app.open_toasts().is_empty());
        assert!(app.reminders().is_empty());
        assert!(!app.signal_active());
    }
}
