//! Renderer consumes this public snapshot boundary without truncating overflow.
mod support;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, TimeSample,
};
use std::sync::{Arc, Mutex};
use support::observed_toast_id;

struct TestClock(Mutex<TimeSample>);
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
fn overflowing_stack_preserves_every_snapshot_order_and_closes_only_chosen_card() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-19T08:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    for index in 0..24 {
        let input: CreateReminder = serde_json::from_value(serde_json::json!({
            "text": format!("Карточка {index}: {}", "Длинное русское напоминание. ".repeat(index * 10 + 1)),
            "schedule": {"kind":"interval", "seconds":60}, "sound": index == 0,
        })).unwrap();
        app.create(input, &context).unwrap();
    }
    {
        let mut now = clock.0.lock().unwrap();
        now.utc += Duration::minutes(1);
        now.awake_millis += 60_000;
    }
    app.tick().unwrap();
    let before = app.open_toasts().to_vec();
    assert_eq!(before.len(), 24);
    assert_eq!(before[0].reminder_id, Some(24));
    assert_eq!(before[23].reminder_id, Some(1));
    {
        let mut now = clock.0.lock().unwrap();
        now.utc += Duration::minutes(1);
        now.awake_millis += 60_000;
    }
    app.tick().unwrap();
    assert_eq!(
        app.open_toasts(),
        before,
        "duplicate occurrence must preserve entry identity and order"
    );
    app.close_toast(observed_toast_id(&app, 13)).unwrap();
    let expected: Vec<_> = before
        .into_iter()
        .filter(|toast| toast.reminder_id != Some(13))
        .collect();
    assert_eq!(app.open_toasts(), expected);
    assert!(app.signal_active());
    app.close_toast(observed_toast_id(&app, 1)).unwrap();
    assert!(!app.signal_active());
    assert_eq!(app.open_toasts().len(), 22);
}

#[test]
fn a_stale_toast_action_cannot_close_a_later_occurrence_of_the_same_reminder() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock(Mutex::new(TimeSample {
        utc: "2026-10-19T08:00:00Z".parse().unwrap(),
        awake_millis: 0,
    })));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let reminder = app
        .create(
            serde_json::from_value(serde_json::json!({
                "text": "Повтор", "schedule": {"kind":"interval", "seconds":60}
            }))
            .unwrap(),
            &CommandContext::capture(clock.as_ref(), "Напомни", None),
        )
        .unwrap();
    let mut now = clock.0.lock().unwrap();
    now.utc += Duration::seconds(60);
    now.awake_millis += 60_000;
    drop(now);
    app.tick().unwrap();
    let first = app.open_toasts()[0].id;
    app.close_toast(first).unwrap();
    let mut now = clock.0.lock().unwrap();
    now.utc += Duration::seconds(60);
    now.awake_millis += 60_000;
    drop(now);
    app.tick().unwrap();
    let later = app.open_toasts()[0].clone();
    assert_eq!(later.reminder_id, Some(reminder.id));
    assert_ne!(later.id, first);
    app.close_toast(first).unwrap();
    app.snooze_toast(first, 5).unwrap();
    assert_eq!(app.open_toasts(), &[later]);
    assert!(app.signal_active());
}
