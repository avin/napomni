use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::{
    mutations::ReminderPatch,
    reminders::{
        Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
    },
};
use std::sync::{Arc, Mutex};
struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new() -> Self {
        Self(Mutex::new(TimeSample {
            utc: "2026-10-18T09:00:00Z".parse().unwrap(),
            awake_millis: 0,
        }))
    }
    fn advance(&self, seconds: i64) {
        let mut now = self.0.lock().unwrap();
        now.utc += Duration::seconds(seconds);
        now.awake_millis += (seconds * 1000) as u64;
    }
}
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        utc.naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![local.and_utc()]
    }
}
fn create(app: &mut Application, sound: Option<bool>, repeat: bool) -> u64 {
    let context = CommandContext::capture(app.clock(), "Создай", None);
    app.create(
        CreateReminder {
            text: "Сигнал".into(),
            schedule: if repeat {
                ScheduleInput::Interval {
                    seconds: 60,
                    start_local: None,
                }
            } else {
                ScheduleInput::AfterSeconds { seconds: 60 }
            },
            border: None,
            sound,
        },
        &context,
    )
    .unwrap()
    .id
}
#[test]
fn one_aggregate_signal_lasts_until_last_sounding_toast_closes_even_when_silent_toast_remains() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let first = create(&mut app, None, false);
    let second = create(&mut app, Some(true), false);
    let silent = create(&mut app, Some(false), false);
    assert!(!app.signal_active());
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 3);
    assert!(app.signal_active());
    app.close_toast(first);
    assert!(app.signal_active());
    app.close_toast(second);
    assert!(!app.signal_active());
    assert_eq!(app.open_toasts()[0].reminder_id, silent);
    app.update(
        silent,
        ReminderPatch {
            sound: Some(true),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Включи звук", Some(silent)),
    )
    .unwrap();
    assert!(!app.signal_active());
    assert!(!app.open_toasts()[0].sound);
    clock.advance(600);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert!(!app.signal_active());
}
#[test]
fn edits_pause_delete_and_undo_keep_current_sound_snapshot_but_future_toast_uses_new_setting() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let id = create(&mut app, None, true);
    clock.advance(60);
    app.tick().unwrap();
    assert!(app.signal_active());
    let snapshot = app.open_toasts()[0].clone();
    app.update(
        id,
        ReminderPatch {
            sound: Some(false),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Без звука", Some(id)),
    )
    .unwrap();
    app.set_paused(id, true).unwrap();
    app.delete(id).unwrap();
    app.undo().unwrap();
    app.set_paused(id, false).unwrap();
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert!(app.signal_active());
    app.close_toast(id);
    assert!(!app.signal_active());
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert!(!app.open_toasts()[0].sound);
    assert!(!app.signal_active());
    let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert!(!app.reminders()[0].sound);
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
}
