use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::{
    external_commands::execute,
    reminders::{Application, Clock, CommandContext, JsonStore, TimeSample},
};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn advance(&self, seconds: i64) {
        let mut now = self.0.lock().unwrap();
        now.utc += Duration::seconds(seconds);
        now.awake_millis += seconds as u64 * 1000;
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
fn immediate_notification_preserves_text_without_a_reminder_and_is_not_restored() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("reminders.json");
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-19T08:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let text = "Unicode: \u{412}\u{43e}\u{434}\u{430} \"quotes\"\nsecond line";
    assert_eq!(
        execute(&mut app, "notify", &json!({"text":text})).unwrap(),
        json!({"ok":true})
    );
    assert!(app.reminders().is_empty());
    assert!(!app.can_undo());
    let toast = &app.open_toasts()[0];
    assert_eq!(toast.text, text);
    assert_eq!(toast.reminder_id, None);
    assert_eq!(toast.border, 0x5285f5);
    assert!(toast.sound);
    assert!(app.signal_active());
    clock.advance(86_400);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].text, text);
    drop(app);
    let app = Application::open(clock, Box::new(JsonStore::new(path))).unwrap();
    assert!(app.reminders().is_empty());
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
}

#[test]
fn duplicate_notifications_share_the_stack_but_close_independently_and_preserve_undo() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-19T08:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("reminders.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "chat", None);
    let reminder = app
        .create(
            serde_json::from_value(json!({
                "text":"Scheduled", "schedule":{"kind":"interval","seconds":60},"sound":false
            }))
            .unwrap(),
            &context,
        )
        .unwrap();
    clock.advance(60);
    app.tick().unwrap();
    let scheduled = app.open_toasts()[0].clone();
    let before = app.reminders().to_vec();
    execute(&mut app, "notify", &json!({"text":"Same text"})).unwrap();
    let first = app.open_toasts()[0].clone();
    execute(&mut app, "notify", &json!({"text":"Same text"})).unwrap();
    let second = app.open_toasts()[0].clone();
    assert_ne!(first.id, second.id);
    assert_eq!(
        app.open_toasts(),
        &[second.clone(), first.clone(), scheduled.clone()]
    );
    assert_eq!(app.reminders(), before);
    assert!(app.signal_active());
    app.close_toast(first.id).unwrap();
    assert_eq!(app.open_toasts(), &[second.clone(), scheduled.clone()]);
    assert!(app.signal_active());
    assert!(
        app.undo().unwrap().is_none(),
        "notification must preserve the undo of chat creation"
    );
    assert!(app.reminders().is_empty());
    assert_eq!(scheduled.reminder_id, Some(reminder.id));
    app.close_toast(second.id).unwrap();
    assert_eq!(app.open_toasts(), &[scheduled]);
    assert!(!app.signal_active());
}

#[test]
fn invalid_notification_arguments_preserve_cards_reminders_and_undo() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = Application::open(
        Arc::new(napomni::reminders::SystemClock),
        Box::new(JsonStore::new(directory.path().join("reminders.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(app.clock(), "chat", None);
    app.create(
        serde_json::from_value(
            json!({"text":"Saved","schedule":{"kind":"after_seconds","seconds":600}}),
        )
        .unwrap(),
        &context,
    )
    .unwrap();
    execute(&mut app, "notify", &json!({"text":"Existing"})).unwrap();
    let cards = app.open_toasts().to_vec();
    let reminders = app.reminders().to_vec();
    for arguments in [
        json!({}),
        json!({"text":"   "}),
        json!({"text":12}),
        json!({"text":"Valid","sound":false}),
        json!({"text":"Valid","schedule":{}}),
        json!({"text":"Valid","sender":"Agent"}),
    ] {
        assert!(execute(&mut app, "notify", &arguments).is_err());
        assert_eq!(app.reminders(), reminders);
        assert_eq!(app.open_toasts(), cards);
        assert!(app.can_undo());
    }
    assert!(app.undo().unwrap().is_none());
    assert!(app.reminders().is_empty());
    assert_eq!(app.open_toasts(), cards);
}
