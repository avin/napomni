use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
};
use std::sync::{Arc, Mutex};

struct TestClock(Mutex<TimeSample>, Mutex<i64>);
impl TestClock {
    fn new() -> Self {
        Self(
            Mutex::new(TimeSample {
                utc: "2026-10-18T09:00:00Z".parse().unwrap(),
                awake_millis: 0,
            }),
            Mutex::new(3),
        )
    }
    fn advance(&self, minutes: i64) {
        let mut now = self.0.lock().unwrap();
        now.utc += Duration::minutes(minutes);
        now.awake_millis += (minutes * 60_000) as u64;
    }
}
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        (utc + Duration::hours(*self.1.lock().unwrap())).naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![(local - Duration::hours(*self.1.lock().unwrap())).and_utc()]
    }
}
#[test]
fn local_one_shot_follows_windows_timezone_but_relative_time_keeps_elapsed_duration() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    app.create(
        CreateReminder {
            text: "По местным часам".into(),
            schedule: ScheduleInput::AtLocal {
                date_time: "2026-10-18T12:20:00".parse().unwrap(),
            },
            border: None,
            sound: None,
        },
        &context,
    )
    .unwrap();
    app.create(
        CreateReminder {
            text: "Через 20 минут".into(),
            schedule: ScheduleInput::AfterSeconds { seconds: 1200 },
            border: None,
            sound: None,
        },
        &context,
    )
    .unwrap();
    *clock.1.lock().unwrap() = 2;
    clock.advance(20);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].text, "Через 20 минут");
    clock.advance(60);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 2);
    assert_eq!(app.open_toasts()[0].text, "По местным часам");
}
#[test]
fn one_shot_survives_restart_then_opens_a_snapshot_and_completes_offline() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("reminders.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(
        clock.as_ref(),
        "Напомни «Ёжик пьёт чай» через 20 минут",
        None,
    );
    let reminder = app
        .create(
            CreateReminder {
                text: "Перефразировано".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 1200 },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap();
    assert_eq!(reminder.text, "Ёжик пьёт чай");
    assert_eq!(reminder.border, 0x5285f5);
    assert!(reminder.sound);
    clock.advance(10);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(app.reminders().len(), 1);
    assert!(app.open_toasts().is_empty());
    clock.advance(10);
    app.tick().unwrap();
    assert!(app.reminders()[0].completed);
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    assert_eq!(app.open_toasts()[0].text, "Ёжик пьёт чай");
    app.close_toast(reminder.id).unwrap();
    assert!(app.open_toasts().is_empty());
    let app = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.reminders()[0].completed);
    assert!(app.open_toasts().is_empty());
}

#[test]
fn delayed_relative_commands_keep_send_time_and_stale_targets_do_not_save() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    clock.advance(5);
    let reminder = app
        .create(
            CreateReminder {
                text: "Вода".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 1200 },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap();
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-18T09:20:00+00:00"
    );
    clock.advance(16);
    let stale = app.create(
        CreateReminder {
            text: "Опоздало".into(),
            schedule: ScheduleInput::AfterSeconds { seconds: 1200 },
            border: None,
            sound: None,
        },
        &context,
    );
    assert!(stale.unwrap_err().to_string().contains("прошло"));
    assert_eq!(app.reminders().len(), 1);
    assert_eq!(
        Application::open(clock, Box::new(JsonStore::new(&path)))
            .unwrap()
            .reminders()
            .len(),
        1
    );
}

#[test]
fn tomorrow_uses_local_submission_date_across_midnight_and_provider_delay() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    clock.advance(11 * 60 + 55); // Local 23:55 on October18.
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни завтра в 10", None);
    clock.advance(10); // The API returns on October19 at00:05.
    let reminder = app
        .create(
            CreateReminder {
                text: "Завтра".into(),
                schedule: ScheduleInput::Tomorrow {
                    time: "10:00:00".parse().unwrap(),
                },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap();
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T07:00:00+00:00"
    );
}

#[test]
fn sleep_and_restart_consume_missed_one_shots_without_opening_toasts() {
    for sleep in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(TestClock::new());
        let path = directory.path().join("data.json");
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
        app.create(
            CreateReminder {
                text: "Пропущено".into(),
                schedule: ScheduleInput::AfterSeconds { seconds: 60 },
                border: None,
                sound: None,
            },
            &context,
        )
        .unwrap();
        if sleep {
            clock.0.lock().unwrap().utc += Duration::minutes(10); // No awake time elapsed.
            app.tick().unwrap();
        } else {
            clock.advance(10);
            app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        }
        assert!(app.open_toasts().is_empty());
        assert!(app.reminders()[0].completed);
        clock.advance(1);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
}

#[test]
fn invalid_input_and_storage_failure_leave_saved_and_visible_data_unchanged() {
    use napomni::reminders::{Persisted, ReminderStore};
    struct FailingStore;
    impl ReminderStore for FailingStore {
        fn load(&self) -> anyhow::Result<Persisted> {
            Ok(Persisted::default())
        }
        fn save(&self, _: &Persisted) -> anyhow::Result<()> {
            anyhow::bail!("Диск недоступен")
        }
    }
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(FailingStore)).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    for seconds in [-1, 0, 60, i64::MAX] {
        assert!(
            app.create(
                CreateReminder {
                    text: "Вода".into(),
                    schedule: ScheduleInput::AfterSeconds { seconds },
                    border: None,
                    sound: None
                },
                &context
            )
            .is_err()
        );
        assert!(app.reminders().is_empty());
        assert!(app.open_toasts().is_empty());
    }
}
