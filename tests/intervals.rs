use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
};
use std::sync::{Arc, Mutex};
struct TestClock {
    now: Mutex<TimeSample>,
    offset: Mutex<i64>,
}
impl TestClock {
    fn new() -> Self {
        Self {
            now: Mutex::new(TimeSample {
                utc: "2026-10-18T09:00:00Z".parse().unwrap(),
                awake_millis: 0,
            }),
            offset: Mutex::new(3),
        }
    }
    fn advance(&self, seconds: i64) {
        let mut now = self.now.lock().unwrap();
        now.utc += Duration::seconds(seconds);
        now.awake_millis += u64::try_from(seconds * 1000).unwrap();
    }
}
impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.now.lock().unwrap()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        (utc + Duration::hours(*self.offset.lock().unwrap())).naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![(local - Duration::hours(*self.offset.lock().unwrap())).and_utc()]
    }
}
fn interval(seconds: i64, start_local: Option<NaiveDateTime>) -> CreateReminder {
    CreateReminder {
        text: "Вода".into(),
        schedule: ScheduleInput::Interval {
            seconds,
            start_local,
        },
        border: None,
        sound: None,
    }
}
#[test]
fn interval_anchors_at_actual_creation_and_keeps_its_grid() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Каждые 10 минут", None);
    clock.advance(180); // API returns at local12:03.
    let reminder = app.create(interval(600, None), &context).unwrap();
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-18T09:13:00+00:00"
    );
    assert!(
        reminder
            .schedule
            .description(clock.as_ref())
            .contains("10 мин")
    );
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    assert!(!app.reminders()[0].completed);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-18T09:23:00+00:00"
    );
}
#[test]
fn open_repeat_is_unchanged_and_close_consumes_missed_repeat_only_for_its_id() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    clock.advance(180);
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    let first = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    let original = app.open_toasts().to_vec();
    clock.advance(600);
    app.tick().unwrap();
    assert_eq!(app.open_toasts(), original);
    let second = app
        .create(
            interval(600, Some("2026-10-18T12:34:00".parse().unwrap())),
            &context,
        )
        .unwrap();
    clock.advance(660);
    app.close_toast(first.id);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(app.open_toasts()[0].reminder_id, second.id);
    clock.advance(539);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 2);
    assert_eq!(app.open_toasts()[0].reminder_id, first.id);
    assert_eq!(app.open_toasts()[0].text, app.open_toasts()[1].text);
}
#[test]
fn sleep_restart_and_timezone_changes_skip_missed_occurrences_and_preserve_phase() {
    for sleep in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let clock = Arc::new(TestClock::new());
        clock.advance(180);
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Каждые 10 минут", None);
        let reminder = app.create(interval(600, None), &context).unwrap();
        clock.advance(600);
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        if sleep {
            app.close_toast(reminder.id);
            clock.now.lock().unwrap().utc += Duration::minutes(35);
            app.tick().unwrap();
        } else {
            clock.advance(2100);
            app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        }
        assert!(app.open_toasts().is_empty());
        assert!(!app.reminders()[0].completed);
        *clock.offset.lock().unwrap() = -4;
        assert_eq!(
            app.reminders()[0]
                .schedule
                .next_after(clock.sample().utc, clock.as_ref())
                .unwrap()
                .to_rfc3339(),
            "2026-10-18T09:53:00+00:00"
        );
        clock.advance(299);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.advance(1);
        app.tick().unwrap();
        assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
        assert_eq!(app.reminders()[0].schedule, reminder.schedule);
    }
}
#[test]
fn explicit_starts_keep_grid_while_invalid_intervals_leave_data_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    clock.advance(180);
    let future = app
        .create(
            interval(600, Some("2026-10-18T12:15:00".parse().unwrap())),
            &context,
        )
        .unwrap();
    let past = app
        .create(
            interval(600, Some("2026-10-18T12:00:00".parse().unwrap())),
            &context,
        )
        .unwrap();
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    for seconds in [-1, 0, i64::MAX] {
        assert!(app.create(interval(seconds, None), &context).is_err());
    }
    assert_eq!(app.reminders().len(), 2);
    clock.advance(420);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, past.id);
    clock.advance(300);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, future.id);
}
