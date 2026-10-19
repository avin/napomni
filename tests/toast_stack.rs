//! Renderer consumes this public snapshot boundary without truncating overflow.
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, TimeSample,
};
use std::sync::{Arc, Mutex};

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
    assert_eq!(before[0].reminder_id, 24);
    assert_eq!(before[23].reminder_id, 1);
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
    app.close_toast(13);
    let expected: Vec<_> = before
        .into_iter()
        .filter(|toast| toast.reminder_id != 13)
        .collect();
    assert_eq!(app.open_toasts(), expected);
    assert!(app.signal_active());
    app.close_toast(1);
    assert!(!app.signal_active());
    assert_eq!(app.open_toasts().len(), 22);
}
