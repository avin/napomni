use chrono::{DateTime, NaiveDateTime, Utc};
use napomni::{
    constraints::Constraints,
    reminders::{
        Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
    },
};
use std::sync::{Arc, Mutex};
struct TestClock(Mutex<TimeSample>);
impl TestClock {
    fn new(at: &str) -> Self {
        Self(Mutex::new(TimeSample {
            utc: at.parse().unwrap(),
            awake_millis: 0,
        }))
    }
    fn set(&self, at: &str) {
        let mut now = self.0.lock().unwrap();
        let utc: DateTime<Utc> = at.parse().unwrap();
        now.awake_millis += u64::try_from((utc - now.utc).num_milliseconds().max(1)).unwrap();
        now.utc = utc;
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
fn input(base: ScheduleInput, limits: Constraints) -> CreateReminder {
    CreateReminder {
        text: "Перерыв".into(),
        schedule: ScheduleInput::Constrained {
            base: Box::new(base),
            limits,
        },
        border: None,
        sound: None,
    }
}
#[test]
fn date_bounds_are_inclusive_and_end_does_not_close_the_existing_toast() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new("2026-10-19T08:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Каждые полчаса с завтра до среды", None);
    let limits = Constraints {
        start_date: Some("2026-10-20".parse().unwrap()),
        end_date: Some("2026-10-21".parse().unwrap()),
        ..Default::default()
    };
    let reminder = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 1800,
                    start_local: None,
                },
                limits,
            ),
            &context,
        )
        .unwrap();
    clock.set("2026-10-19T23:59:59Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.set("2026-10-20T00:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    app.close_toast(reminder.id).unwrap();
    clock.set("2026-10-21T23:30:00Z");
    app.tick().unwrap();
    let last = app.open_toasts().to_vec();
    clock.set("2026-10-22T00:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts(), last);
    assert!(app.reminders()[0].completed);
    app.close_toast(reminder.id).unwrap();
    clock.set("2026-10-23T00:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(
        reminder
            .schedule
            .description(clock.as_ref())
            .contains("включительно")
    );
}
#[test]
fn half_hour_weekday_window_exclusion_and_restart_preserve_the_original_phase() {
    use napomni::constraints::TimeWindow;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.json");
    let clock = Arc::new(TestClock::new("2026-10-29T08:30:00Z"));
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(
        clock.as_ref(),
        "Каждые полчаса с 9 до 18 по будням кроме 30 октября",
        None,
    );
    let limits = Constraints {
        start_date: Some("2026-10-29".parse().unwrap()),
        end_date: Some("2026-11-02".parse().unwrap()),
        window: Some(TimeWindow {
            start: "09:00:00".parse().unwrap(),
            end: "18:00:00".parse().unwrap(),
        }),
        weekdays: Some(vec![1, 2, 3, 4, 5]),
        excluded_dates: vec!["2026-10-30".parse().unwrap()],
    };
    let reminder = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 1800,
                    start_local: None,
                },
                limits,
            ),
            &context,
        )
        .unwrap();
    clock.set("2026-10-29T08:59:59Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.set("2026-10-29T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    app.close_toast(reminder.id).unwrap();
    clock.set("2026-10-29T18:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    app.close_toast(reminder.id).unwrap();
    for at in [
        "2026-10-29T18:00:01Z",
        "2026-10-30T09:00:00Z",
        "2026-10-31T09:00:00Z",
    ] {
        clock.set(at);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
    clock.set("2026-11-01T09:00:00Z");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(app.reminders()[0].schedule, reminder.schedule);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-11-02T09:00:00+00:00"
    );
    clock.set("2026-11-02T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    let phase_limits = Constraints {
        window: Some(TimeWindow {
            start: "09:00:00".parse().unwrap(),
            end: "18:00:00".parse().unwrap(),
        }),
        ..Default::default()
    };
    let phase = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 1800,
                    start_local: Some("2026-11-02T08:07:00".parse().unwrap()),
                },
                phase_limits,
            ),
            &context,
        )
        .unwrap();
    assert_eq!(
        phase
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-11-02T09:07:00+00:00"
    );
}
#[test]
fn limits_filter_weekly_monthly_and_yearly_without_changing_their_calendar_phase() {
    use napomni::{calendar_dates::YearlyDate, constraints::TimeWindow};
    let cases = [
        (
            ScheduleInput::Weekly {
                weekdays: vec![1, 3, 5],
                times: vec!["08:00:00".parse().unwrap(), "10:00:00".parse().unwrap()],
                every_n_weeks: 2,
                anchor_monday: Some("2026-10-19".parse().unwrap()),
            },
            "2026-10-19T07:00:00Z",
            "2026-10-19T10:00:00+00:00",
        ),
        (
            ScheduleInput::Monthly {
                days: vec![1, 15],
                times: vec!["08:00:00".parse().unwrap(), "10:00:00".parse().unwrap()],
            },
            "2026-10-31T07:00:00Z",
            "2026-12-01T10:00:00+00:00",
        ),
        (
            ScheduleInput::Yearly {
                dates: vec![
                    YearlyDate { month: 10, day: 31 },
                    YearlyDate { month: 11, day: 1 },
                    YearlyDate { month: 12, day: 1 },
                ],
                times: vec!["08:00:00".parse().unwrap(), "10:00:00".parse().unwrap()],
            },
            "2026-10-31T07:00:00Z",
            "2026-12-01T10:00:00+00:00",
        ),
    ];
    for (base, at, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(TestClock::new(at));
        let mut app = Application::open(
            clock.clone(),
            Box::new(JsonStore::new(dir.path().join("data.json"))),
        )
        .unwrap();
        let context = CommandContext::capture(clock.as_ref(), "По будням в рабочие часы", None);
        let limits = Constraints {
            window: Some(TimeWindow {
                start: "09:00:00".parse().unwrap(),
                end: "18:00:00".parse().unwrap(),
            }),
            weekdays: Some(vec![1, 2, 3, 4, 5]),
            excluded_dates: vec!["2026-10-21".parse().unwrap()],
            ..Default::default()
        };
        let reminder = app.create(input(base, limits), &context).unwrap();
        assert_eq!(
            reminder
                .schedule
                .next_after(clock.sample().utc, clock.as_ref())
                .unwrap()
                .to_rfc3339(),
            expected
        );
        clock.set(&expected.replace("+00:00", "Z"));
        app.tick().unwrap();
        assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
        if expected.starts_with("2026-10-19") {
            assert_eq!(
                reminder
                    .schedule
                    .next_after(clock.sample().utc, clock.as_ref())
                    .unwrap()
                    .to_rfc3339(),
                "2026-10-23T10:00:00+00:00"
            );
        }
    }
}
#[test]
fn broader_interval_combinations_work_and_impossible_or_unsupported_rules_are_errors() {
    use napomni::constraints::TimeWindow;
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new("2026-10-16T07:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    let narrow = Constraints {
        window: Some(TimeWindow {
            start: "09:00:00".parse().unwrap(),
            end: "10:00:00".parse().unwrap(),
        }),
        ..Default::default()
    };
    let two_hours = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 7200,
                    start_local: Some("2026-10-16T07:30:00".parse().unwrap()),
                },
                narrow.clone(),
            ),
            &context,
        )
        .unwrap();
    assert_eq!(
        two_hours
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-16T09:30:00+00:00"
    );
    let two_days = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 172800,
                    start_local: Some("2026-10-17T12:00:00".parse().unwrap()),
                },
                Constraints {
                    weekdays: Some(vec![1, 2, 3, 4, 5]),
                    ..Default::default()
                },
            ),
            &context,
        )
        .unwrap();
    assert_eq!(
        two_days
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T12:00:00+00:00"
    );
    let impossible = app
        .create(
            input(
                ScheduleInput::Interval {
                    seconds: 86400,
                    start_local: Some("2026-10-16T20:00:00".parse().unwrap()),
                },
                narrow,
            ),
            &context,
        )
        .unwrap_err();
    assert!(
        impossible
            .to_string()
            .contains("не даёт будущего срабатывания")
    );
    let overnight = Constraints {
        window: Some(TimeWindow {
            start: "18:00:00".parse().unwrap(),
            end: "09:00:00".parse().unwrap(),
        }),
        ..Default::default()
    };
    assert!(
        app.create(
            input(
                ScheduleInput::Interval {
                    seconds: 1800,
                    start_local: None
                },
                overnight
            ),
            &context
        )
        .unwrap_err()
        .to_string()
        .contains("полночь")
    );
    let reversed = Constraints {
        start_date: Some("2026-11-01".parse().unwrap()),
        end_date: Some("2026-10-01".parse().unwrap()),
        ..Default::default()
    };
    assert!(
        app.create(
            input(
                ScheduleInput::Interval {
                    seconds: 1800,
                    start_local: None
                },
                reversed
            ),
            &context
        )
        .is_err()
    );
    assert!(
        app.create(
            input(
                ScheduleInput::AfterSeconds { seconds: 60 },
                Constraints::default()
            ),
            &context
        )
        .is_err()
    );
    assert_eq!(app.reminders().len(), 2);
}
#[test]
fn changing_only_limits_preserves_base_grid_and_other_properties_and_is_undoable() {
    use napomni::{constraints::TimeWindow, mutations::ReminderPatch};
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new("2026-10-19T08:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Каждые 10 минут", None);
    let reminder = app
        .create(
            CreateReminder {
                text: "Вода".into(),
                schedule: ScheduleInput::Interval {
                    seconds: 600,
                    start_local: None,
                },
                border: Some(0x00ff00),
                sound: Some(false),
            },
            &context,
        )
        .unwrap();
    clock.set("2026-10-19T08:03:00Z");
    let limits = Constraints {
        window: Some(TimeWindow {
            start: "09:00:00".parse().unwrap(),
            end: "18:00:00".parse().unwrap(),
        }),
        ..Default::default()
    };
    let updated = app
        .update(
            reminder.id,
            ReminderPatch {
                limits: Some(limits.clone()),
                ..Default::default()
            },
            &context,
        )
        .unwrap();
    assert_eq!(
        updated
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T09:00:00+00:00"
    );
    assert_eq!(updated.text, reminder.text);
    assert_eq!(updated.border, reminder.border);
    assert_eq!(updated.sound, reminder.sound);
    clock.set("2026-10-19T09:00:00Z");
    app.tick().unwrap();
    let snapshot = app.open_toasts().to_vec();
    app.update(
        reminder.id,
        ReminderPatch {
            limits: Some(Constraints {
                excluded_dates: vec!["2026-10-19".parse().unwrap()],
                ..limits
            }),
            ..Default::default()
        },
        &context,
    )
    .unwrap();
    assert_eq!(app.open_toasts(), snapshot);
    app.undo().unwrap();
    assert_eq!(app.open_toasts(), snapshot);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T09:10:00+00:00"
    );
    clock.set("2026-10-19T18:03:00Z");
    let clear: ReminderPatch = serde_json::from_value(serde_json::json!({"limits":{}})).unwrap();
    let cleared = app.update(reminder.id, clear, &context).unwrap();
    assert_eq!(
        cleared
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-19T18:10:00+00:00"
    );
    assert_eq!(cleared.text, reminder.text);
    assert_eq!(cleared.border, reminder.border);
    assert_eq!(cleared.sound, reminder.sound);
    assert_eq!(app.open_toasts(), snapshot);
    app.undo().unwrap();
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2026-10-20T09:00:00+00:00"
    );
}
