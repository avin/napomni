use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use std::sync::{Arc, Mutex};
use win_notifi_toast::reminders::{
    Application, Clock, CommandContext, CreateReminder, JsonStore, ScheduleInput, TimeSample,
};
struct CalendarClock {
    now: Mutex<TimeSample>,
    offset: Mutex<i64>,
}
impl CalendarClock {
    fn new(at: &str) -> Self {
        Self {
            now: Mutex::new(TimeSample {
                utc: at.parse().unwrap(),
                awake_millis: 0,
            }),
            offset: Mutex::new(3),
        }
    }
    fn set(&self, at: &str) {
        let mut now = self.now.lock().unwrap();
        let utc: DateTime<Utc> = at.parse().unwrap();
        let elapsed = (utc - now.utc).num_milliseconds();
        now.awake_millis += u64::try_from(elapsed.max(1)).unwrap();
        now.utc = utc;
    }
}
impl Clock for CalendarClock {
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
fn input(schedule: ScheduleInput) -> CreateReminder {
    CreateReminder {
        text: "Календарь".into(),
        schedule,
        border: None,
        sound: None,
    }
}
#[test]
fn monthly_31_skips_short_months_and_continues_through_year_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(CalendarClock::new("2027-01-30T09:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Каждое 31-е в 12", None);
    let reminder = app
        .create(
            input(ScheduleInput::Monthly {
                days: vec![31],
                times: vec!["12:00:00".parse().unwrap()],
            }),
            &context,
        )
        .unwrap();
    clock.set("2027-01-31T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    app.close_toast(reminder.id);
    clock.set("2027-02-28T09:00:00Z");
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2027-03-31T09:00:00+00:00"
    );
    clock.set("2027-03-31T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert!(!app.reminders()[0].completed);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after("2027-12-31T09:00:00Z".parse().unwrap(), clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2028-01-31T09:00:00+00:00"
    );
    assert!(
        app.reminders()[0]
            .schedule
            .description(clock.as_ref())
            .contains("31")
    );
}
#[test]
fn yearly_february_29_occurs_only_in_leap_years_and_invalid_dates_are_rejected() {
    use win_notifi_toast::calendar_dates::YearlyDate;
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(CalendarClock::new("2027-02-28T09:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Каждое 29 февраля в 12", None);
    let reminder = app
        .create(
            input(ScheduleInput::Yearly {
                dates: vec![YearlyDate { month: 2, day: 29 }],
                times: vec!["12:00:00".parse().unwrap()],
            }),
            &context,
        )
        .unwrap();
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2028-02-29T09:00:00+00:00"
    );
    clock.set("2028-02-29T09:00:00Z");
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, reminder.id);
    app.close_toast(reminder.id);
    assert_eq!(
        reminder
            .schedule
            .next_after(clock.sample().utc, clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2032-02-29T09:00:00+00:00"
    );
    assert_eq!(
        reminder
            .schedule
            .next_after("2096-02-29T09:00:00Z".parse().unwrap(), clock.as_ref())
            .unwrap()
            .to_rfc3339(),
        "2104-02-29T09:00:00+00:00"
    );
    for (month, day) in [(2, 30), (4, 31), (0, 1), (13, 1)] {
        assert!(
            app.create(
                input(ScheduleInput::Yearly {
                    dates: vec![YearlyDate { month, day }],
                    times: vec!["12:00:00".parse().unwrap()]
                }),
                &context
            )
            .is_err()
        );
    }
    assert_eq!(app.reminders().len(), 1);
}
struct TransitionClock(Mutex<TimeSample>);
impl TransitionClock {
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
impl Clock for TransitionClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }
    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        let summer = utc >= "2028-03-26T01:00:00Z".parse::<DateTime<Utc>>().unwrap()
            && utc < "2028-10-29T01:00:00Z".parse::<DateTime<Utc>>().unwrap();
        (utc + Duration::hours(i64::from(summer))).naive_utc()
    }
    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        [local.and_utc(), (local - Duration::hours(1)).and_utc()]
            .into_iter()
            .filter(|at| self.local_at(*at) == local)
            .collect()
    }
}
#[test]
fn monthly_and_yearly_skip_dst_gaps_and_never_replay_either_fold_slot() {
    use win_notifi_toast::calendar_dates::YearlyDate;
    for yearly in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        let clock = Arc::new(TransitionClock::new("2028-03-25T10:00:00Z"));
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
        let schedule = if yearly {
            ScheduleInput::Yearly {
                dates: vec![YearlyDate { month: 3, day: 26 }],
                times: vec!["01:30:00".parse().unwrap()],
            }
        } else {
            ScheduleInput::Monthly {
                days: vec![26],
                times: vec!["01:30:00".parse().unwrap()],
            }
        };
        let reminder = app.create(input(schedule), &context).unwrap();
        let expected = if yearly {
            "2029-03-26T01:30:00+00:00"
        } else {
            "2028-04-26T00:30:00+00:00"
        };
        assert_eq!(
            reminder
                .schedule
                .next_after(clock.sample().utc, clock.as_ref())
                .unwrap()
                .to_rfc3339(),
            expected
        );
        clock.set("2028-03-26T02:00:00Z");
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        let clock = Arc::new(TransitionClock::new("2028-10-28T23:00:00Z"));
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
        let times = vec!["01:10:00".parse().unwrap(), "01:40:00".parse().unwrap()];
        let schedule = if yearly {
            ScheduleInput::Yearly {
                dates: vec![YearlyDate { month: 10, day: 29 }],
                times,
            }
        } else {
            ScheduleInput::Monthly {
                days: vec![29],
                times,
            }
        };
        let reminder = app.create(input(schedule), &context).unwrap();
        clock.set("2028-10-29T00:10:00Z");
        app.tick().unwrap();
        let first = app.open_toasts().to_vec();
        clock.set("2028-10-29T00:40:00Z");
        app.tick().unwrap();
        assert_eq!(app.open_toasts(), first);
        app.close_toast(reminder.id);
        clock.set("2028-10-29T01:05:00Z");
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        clock.set("2028-10-29T01:10:00Z");
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.set("2028-10-29T01:40:00Z");
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
}
#[test]
fn calendar_rules_persist_skip_offline_dates_and_keep_multiple_local_slots_after_timezone_change() {
    use win_notifi_toast::calendar_dates::YearlyDate;
    for yearly in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        let clock = Arc::new(CalendarClock::new("2027-12-31T20:00:00Z"));
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
        let times = vec!["09:00:00".parse().unwrap(), "18:00:00".parse().unwrap()];
        let schedule = if yearly {
            ScheduleInput::Yearly {
                dates: vec![
                    YearlyDate { month: 1, day: 1 },
                    YearlyDate { month: 1, day: 15 },
                ],
                times,
            }
        } else {
            ScheduleInput::Monthly {
                days: vec![1, 15],
                times,
            }
        };
        let reminder = app.create(input(schedule), &context).unwrap();
        assert_eq!(
            reminder
                .schedule
                .next_after(clock.sample().utc, clock.as_ref())
                .unwrap()
                .to_rfc3339(),
            "2028-01-01T06:00:00+00:00"
        );
        clock.set("2028-01-02T09:00:00Z");
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        assert_eq!(app.reminders()[0].schedule, reminder.schedule);
        *clock.offset.lock().unwrap() = 2;
        assert_eq!(
            app.reminders()[0]
                .schedule
                .next_after(clock.sample().utc, clock.as_ref())
                .unwrap()
                .to_rfc3339(),
            "2028-01-15T07:00:00+00:00"
        );
        clock.set("2028-01-15T06:00:00Z");
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.set("2028-01-15T07:00:00Z");
        app.tick().unwrap();
        let first = app.open_toasts().to_vec();
        assert_eq!(first[0].reminder_id, reminder.id);
        clock.set("2028-01-15T16:00:00Z");
        app.tick().unwrap();
        assert_eq!(app.open_toasts(), first);
        assert!(!app.reminders()[0].completed);
    }
}
#[test]
fn invalid_calendar_sets_do_not_create_a_reminder() {
    use win_notifi_toast::calendar_dates::YearlyDate;
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(CalendarClock::new("2027-01-01T00:00:00Z"));
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(dir.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Напомни", None);
    for schedule in [
        ScheduleInput::Monthly {
            days: vec![],
            times: vec!["12:00:00".parse().unwrap()],
        },
        ScheduleInput::Monthly {
            days: vec![0, 32],
            times: vec!["12:00:00".parse().unwrap()],
        },
        ScheduleInput::Monthly {
            days: vec![1],
            times: vec![],
        },
        ScheduleInput::Yearly {
            dates: vec![],
            times: vec!["12:00:00".parse().unwrap()],
        },
        ScheduleInput::Yearly {
            dates: vec![YearlyDate { month: 1, day: 1 }],
            times: vec![],
        },
    ] {
        assert!(app.create(input(schedule), &context).is_err());
    }
    assert!(app.reminders().is_empty());
    assert!(app.open_toasts().is_empty());
}
