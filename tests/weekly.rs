use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::sync::{Arc, Mutex};
use win_notifi_toast::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, TimeSample,
};

struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new(at: &str) -> Self {
        Self(Mutex::new(TimeSample {
            utc: at.parse().unwrap(),
            awake_millis: 0,
        }))
    }
    fn advance_to(&self, at: &str) {
        let utc: DateTime<Utc> = at.parse().unwrap();
        let mut sample = self.0.lock().unwrap();
        sample.awake_millis += (utc - sample.utc).num_milliseconds().max(1) as u64;
        sample.utc = utc;
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
fn input(schedule: serde_json::Value) -> CreateReminder {
    serde_json::from_value(serde_json::json!({"text":"Размяться", "schedule":schedule})).unwrap()
}

#[test]
fn weekly_selected_days_and_multiple_times_open_future_toasts_after_each_close() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new("2026-10-19T08:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context =
        CommandContext::capture(clock.as_ref(), "По понедельникам и средам в 9 и 10", None);
    let reminder = app.create(input(serde_json::json!({"kind":"weekly", "weekdays":[1,3], "times":["09:00:00","10:00:00"], "every_n_weeks":1})), &context).unwrap();
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T09:00:00+00:00"
    );
    clock.advance_to("2026-10-19T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    app.close_toast(reminder.id);
    clock.advance_to("2026-10-19T10:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    assert!(!app.reminders()[0].completed);
    app.close_toast(reminder.id);
    clock.advance_to("2026-10-20T10:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance_to("2026-10-21T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].text, "Размяться");
}

#[test]
fn two_week_phase_uses_mondays_across_year_and_survives_restart_without_catchup() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock::new("2026-12-28T08:00:00Z"));
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(
        clock.as_ref(),
        "По вторникам и четвергам раз в две недели начиная с 28 декабря",
        None,
    );
    let reminder = app.create(input(serde_json::json!({"kind":"weekly", "weekdays":[2,4], "times":["09:00:00"], "every_n_weeks":2,"anchor_monday":"2026-12-28"})), &context).unwrap();
    clock.advance_to("2026-12-29T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    // Keep the snapshot open: the second occurrence must not move it or replay entry.
    let snapshot = app.open_toasts().to_vec();
    clock.advance_to("2026-12-31T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts(), snapshot);
    app.close_toast(reminder.id);
    clock.advance_to("2027-01-05T09:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    drop(app);
    clock.advance_to("2027-01-12T09:05:00Z");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2027-01-14T09:00:00+00:00"
    );
    clock.advance_to("2027-01-14T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
}

#[test]
fn invalid_or_ambiguous_weekly_rules_leave_saved_reminders_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new("2026-10-19T08:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    for schedule in [
        serde_json::json!({"kind":"weekly","weekdays":[2,4],"times":["09:00:00"],"every_n_weeks":2}),
        serde_json::json!({"kind":"weekly","weekdays":[2],"times":["09:00:00"],"every_n_weeks":0}),
        serde_json::json!({"kind":"weekly","weekdays":[0,8],"times":["09:00:00"],"every_n_weeks":1}),
        serde_json::json!({"kind":"weekly","weekdays":[],"times":["09:00:00"],"every_n_weeks":1}),
        serde_json::json!({"kind":"weekly","weekdays":[1],"times":[],"every_n_weeks":1}),
        serde_json::json!({"kind":"weekly","weekdays":[1],"times":["09:00:00"],"every_n_weeks":2,"anchor_monday":"2026-10-20"}),
    ] {
        assert!(app.create(input(schedule), &context).is_err());
        assert!(app.reminders().is_empty());
    }
}

/// Replaceable Windows time-zone boundary with real 2026 European gap/fold instants.
struct ZonedClock {
    clock: TestClock,
    fixed_offset: Mutex<Option<i64>>,
}
impl ZonedClock {
    fn new(at: &str) -> Self {
        Self {
            clock: TestClock::new(at),
            fixed_offset: Mutex::new(None),
        }
    }
    fn advance_to(&self, at: &str) {
        self.clock.advance_to(at);
    }
}
impl Clock for ZonedClock {
    fn sample(&self) -> TimeSample {
        self.clock.sample()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        let offset = self.fixed_offset.lock().unwrap().unwrap_or_else(|| {
            if utc >= "2026-03-29T01:00:00Z".parse::<DateTime<Utc>>().unwrap()
                && utc < "2026-10-25T01:00:00Z".parse::<DateTime<Utc>>().unwrap()
            {
                2
            } else {
                1
            }
        });
        (utc + Duration::hours(offset)).naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        let offsets = if let Some(offset) = *self.fixed_offset.lock().unwrap() {
            vec![offset]
        } else {
            vec![1, 2]
        };
        offsets
            .into_iter()
            .map(|offset| (local - Duration::hours(offset)).and_utc())
            .filter(|at| self.local_at(*at) == local)
            .collect()
    }
}

#[test]
fn spring_gap_is_skipped_and_next_scheduled_time_still_fires() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(ZonedClock::new("2026-03-29T00:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "По воскресеньям в 02:30 и 03:30", None);
    app.create(input(serde_json::json!({"kind":"weekly","weekdays":[7],"times":["02:30:00","03:30:00"],"every_n_weeks":1})), &context).unwrap();
    clock.advance_to("2026-03-29T01:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance_to("2026-03-29T01:30:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-04-05T00:30:00+00:00"
    );
}

#[test]
fn autumn_fold_does_not_repeat_either_of_two_local_slots_even_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(ZonedClock::new("2026-10-24T23:00:00Z"));
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "По воскресеньям в 02:15 и 02:45", None);
    let reminder = app.create(input(serde_json::json!({"kind":"weekly","weekdays":[7],"times":["02:15:00","02:45:00"],"every_n_weeks":1})), &context).unwrap();
    for instant in ["2026-10-25T00:15:00Z", "2026-10-25T00:45:00Z"] {
        clock.advance_to(instant);
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        app.close_toast(reminder.id);
    }
    drop(app);
    clock.advance_to("2026-10-25T01:00:00Z");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    for instant in ["2026-10-25T01:15:00Z", "2026-10-25T01:45:00Z"] {
        clock.advance_to(instant);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-11-01T01:15:00+00:00"
    );
}

#[test]
fn weekly_calendar_follows_current_windows_timezone() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(ZonedClock::new("2026-10-19T06:00:00Z"));
    *clock.fixed_offset.lock().unwrap() = Some(3);
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "В понедельник в 10", None);
    app.create(input(serde_json::json!({"kind":"weekly","weekdays":[1],"times":["10:00:00"],"every_n_weeks":1})), &context).unwrap();
    *clock.fixed_offset.lock().unwrap() = Some(2);
    clock.advance_to("2026-10-19T07:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance_to("2026-10-19T08:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
}

#[test]
fn manual_clock_rollback_does_not_replay_two_consumed_local_slots_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock::new("2026-10-19T08:00:00Z"));
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "По понедельникам в 9 и 10", None);
    let reminder = app.create(input(serde_json::json!({"kind":"weekly","weekdays":[1],"times":["09:00:00","10:00:00"],"every_n_weeks":1})), &context).unwrap();
    for instant in ["2026-10-19T09:00:00Z", "2026-10-19T10:00:00Z"] {
        clock.advance_to(instant);
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        app.close_toast(reminder.id);
    }
    clock.advance_to("2026-10-19T08:30:00Z");
    app.tick().unwrap();
    drop(app);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    for instant in ["2026-10-19T09:00:00Z", "2026-10-19T10:00:00Z"] {
        clock.advance_to(instant);
        app.tick().unwrap();
        assert!(
            app.open_toasts().is_empty(),
            "consumed local slots must not replay after rollback"
        );
    }
    clock.advance_to("2026-10-26T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
}
