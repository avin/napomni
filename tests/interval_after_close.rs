mod support;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::{
    reminder_tools::{execute_tool, tools},
    reminders::{
        Application, Clock, CommandContext, CreateReminder, JsonStore, Schedule, ScheduleInput,
        TimeSample,
    },
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use support::observed_toast_id;

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
        now.awake_millis += u64::try_from(seconds * 1000).unwrap();
    }

    fn sleep(&self, seconds: i64) {
        self.0.lock().unwrap().utc += Duration::seconds(seconds);
    }
}

impl Clock for TestClock {
    fn sample(&self) -> TimeSample {
        *self.0.lock().unwrap()
    }

    fn local_at(&self, utc: DateTime<Utc>) -> NaiveDateTime {
        (utc + Duration::hours(3)).naive_utc()
    }

    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![(local - Duration::hours(3)).and_utc()]
    }
}

fn interval(seconds: i64, start_local: Option<NaiveDateTime>) -> CreateReminder {
    CreateReminder {
        text: "Размяться".into(),
        schedule: ScheduleInput::IntervalAfterClose {
            seconds,
            start_local,
        },
        border: None,
        sound: None,
    }
}

fn next(app: &Application, id: u64) -> Option<DateTime<Utc>> {
    match &app
        .reminders()
        .iter()
        .find(|r| r.id == id)
        .unwrap()
        .schedule
    {
        Schedule::IntervalAfterClose { next_utc, .. } => *next_utc,
        other => panic!("Unexpected schedule: {other:?}"),
    }
}

#[test]
fn first_occurrence_uses_actual_creation_and_close_starts_a_full_new_interval() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    clock.advance(180);
    let reminder = app.create(interval(600, None), &context).unwrap();
    assert_eq!(
        next(&app, reminder.id),
        Some(clock.sample().utc + Duration::minutes(10))
    );
    assert!(
        reminder
            .schedule
            .description(clock.as_ref())
            .starts_with("Через 10 мин после закрытия")
    );
    assert!(
        app.schedule_status(reminder.id)
            .unwrap()
            .starts_with("Следующее")
    );
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    assert_eq!(snapshot.reminder_id, Some(reminder.id));
    assert_eq!(next(&app, reminder.id), None);
    assert_eq!(
        app.schedule_status(reminder.id).as_deref(),
        Some("Ожидает закрытия уведомления")
    );
    clock.advance(1500);
    app.tick().unwrap();
    assert_eq!(app.open_toasts(), &[snapshot.clone()]);
    assert!(!app.reminders()[0].completed);
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    let expected = clock.sample().utc + Duration::minutes(10);
    assert_eq!(next(&app, reminder.id), Some(expected));
    clock.advance(120);
    app.close_toast(snapshot.id).unwrap();
    assert_eq!(next(&app, reminder.id), Some(expected));
    clock.advance(479);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn future_explicit_start_and_invalid_periods_are_validated_before_saving() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app
        .create(
            interval(600, Some("2026-10-18T12:15:00".parse().unwrap())),
            &context,
        )
        .unwrap();
    assert_eq!(
        next(&app, reminder.id),
        Some("2026-10-18T09:15:00Z".parse().unwrap())
    );
    for seconds in [-1, 0, i64::MAX] {
        assert!(app.create(interval(seconds, None), &context).is_err());
    }
    assert_eq!(app.reminders().len(), 1);
    clock.advance(899);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn waiting_state_survives_sleep_and_restart_restarts_without_a_phantom_toast() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    clock.sleep(3600);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert_eq!(next(&app, reminder.id), None);
    drop(app);
    clock.advance(120);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(
        next(&app, reminder.id),
        Some(clock.sample().utc + Duration::minutes(10))
    );
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn close_persists_the_next_occurrence_and_restart_starts_a_full_interval() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    clock.advance(300);
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    drop(app);
    clock.advance(120);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    assert_eq!(
        next(&app, reminder.id),
        Some(clock.sample().utc + Duration::minutes(10))
    );
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn pausing_or_deleting_an_open_reminder_does_not_reopen_its_toast() {
    for delete in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(
            clock.clone(),
            Box::new(JsonStore::new(directory.path().join("data.json"))),
        )
        .unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Создай", None);
        let reminder = app.create(interval(600, None), &context).unwrap();
        clock.advance(600);
        app.tick().unwrap();
        if delete {
            app.delete(reminder.id).unwrap();
        } else {
            app.set_paused(reminder.id, true).unwrap();
        }
        assert_eq!(app.open_toasts().len(), 1);
        app.close_toast(observed_toast_id(&app, reminder.id))
            .unwrap();
        clock.advance(1200);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        if delete {
            assert!(app.reminders().is_empty());
        } else {
            assert!(app.reminders()[0].paused);
            assert!(!app.reminders()[0].completed);
        }
    }
}

#[test]
fn tools_expose_create_and_edit_for_the_opt_in_schedule_and_keep_plain_interval_default() {
    let schemas = serde_json::to_value(tools()).unwrap();
    for name in ["create_reminder", "update_reminder"] {
        let tool = schemas
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["function"]["name"] == name)
            .unwrap();
        let properties = &tool["function"]["parameters"]["properties"];
        let schedule = if name == "create_reminder" {
            &properties["schedule"]
        } else {
            &properties["patch"]["properties"]["schedule"]
        };
        assert!(
            schedule.to_string().contains("interval_after_close"),
            "Missing schedule in {name}"
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let path = directory.path().join("data.json");
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let created = execute_tool(
        &mut app,
        "create_reminder",
        &json!({"text":"Размяться","schedule":{"kind":"interval_after_close","seconds":600}})
            .to_string(),
        &context,
    )
    .unwrap();
    assert_eq!(
        created["reminder"]["schedule"]["kind"],
        "interval_after_close"
    );
    assert_eq!(created["reminder"]["schedule"]["period_seconds"], 600);
    let id = app.reminders()[0].id;
    clock.advance(600);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    let edit = CommandContext::capture(clock.as_ref(), "Измени", Some(id));
    execute_tool(
        &mut app,
        "update_reminder",
        &json!({"id":id,"patch":{"schedule":{"kind":"interval","seconds":600}}}).to_string(),
        &edit,
    )
    .unwrap();
    assert!(matches!(
        app.reminders()[0].schedule,
        Schedule::Interval { .. }
    ));
    assert_eq!(app.open_toasts(), &[snapshot]);
    clock.advance(120);
    app.close_toast(observed_toast_id(&app, id)).unwrap();
    clock.advance(480);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(id));
    let reopened = Application::open(clock, Box::new(JsonStore::new(&path))).unwrap();
    assert!(matches!(
        reopened.reminders()[0].schedule,
        Schedule::Interval { .. }
    ));
}

#[test]
fn constrained_occurrences_filter_the_new_phase_anchored_at_closure() {
    use napomni::constraints::{Constraints, TimeWindow};
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let mut input = interval(600, None);
    input.schedule = ScheduleInput::Constrained {
        base: Box::new(input.schedule),
        limits: Constraints {
            window: Some(TimeWindow {
                start: "12:00:00".parse().unwrap(),
                end: "12:20:00".parse().unwrap(),
            }),
            ..Default::default()
        },
    };
    let reminder = app.create(input, &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    clock.advance(420); // Closed at local 12:17; new phase starts at 12:27.
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref()),
        Some("2026-10-19T09:07:00Z".parse().unwrap())
    );
    clock.advance(600);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(23 * 3600 + 40 * 60 - 1);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn editing_only_limits_while_waiting_preserves_waiting_until_close() {
    use napomni::{
        constraints::{Constraints, TimeWindow},
        mutations::ReminderPatch,
    };
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    app.update(
        reminder.id,
        ReminderPatch {
            limits: Some(Constraints {
                window: Some(TimeWindow {
                    start: "12:00:00".parse().unwrap(),
                    end: "18:00:00".parse().unwrap(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Измени ограничения", Some(reminder.id)),
    )
    .unwrap();
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref()),
        None
    );
    assert!(!app.reminders()[0].completed);
    clock.advance(300);
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    assert_eq!(
        app.reminders()[0]
            .schedule
            .next_after(clock.sample().utc, clock.as_ref()),
        Some(clock.sample().utc + Duration::minutes(10))
    );
}

#[test]
fn failed_close_save_preserves_the_open_toast_and_waiting_state_until_retry() {
    use napomni::reminders::{Persisted, ReminderStore};
    use std::sync::atomic::{AtomicBool, Ordering};
    struct FailableStore {
        store: JsonStore,
        fail: Arc<AtomicBool>,
    }
    impl ReminderStore for FailableStore {
        fn load(&self) -> anyhow::Result<Persisted> {
            self.store.load()
        }
        fn save(&self, state: &Persisted) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail.load(Ordering::SeqCst), "Диск недоступен");
            self.store.save(state)
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let fail = Arc::new(AtomicBool::new(false));
    let mut app = Application::open(
        clock.clone(),
        Box::new(FailableStore {
            store: JsonStore::new(directory.path().join("data.json")),
            fail: fail.clone(),
        }),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    fail.store(true, Ordering::SeqCst);
    assert!(
        app.close_toast(observed_toast_id(&app, reminder.id))
            .is_err()
    );
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert_eq!(next(&app, reminder.id), None);
    clock.advance(120);
    fail.store(false, Ordering::SeqCst);
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    assert!(app.open_toasts().is_empty());
    assert_eq!(
        next(&app, reminder.id),
        Some(clock.sample().utc + Duration::minutes(10))
    );
}

#[test]
fn undoing_a_property_edit_after_close_keeps_the_new_deadline() {
    use napomni::mutations::ReminderPatch;
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let reminder = app.create(interval(600, None), &context).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    app.update(
        reminder.id,
        ReminderPatch {
            text: Some("Новый текст".into()),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Измени текст", Some(reminder.id)),
    )
    .unwrap();
    clock.advance(180);
    app.close_toast(observed_toast_id(&app, reminder.id))
        .unwrap();
    let expected = clock.sample().utc + Duration::minutes(10);
    clock.advance(120);
    app.undo().unwrap();
    assert_eq!(app.reminders()[0].text, reminder.text);
    assert_eq!(next(&app, reminder.id), Some(expected));
    clock.advance(479);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
}

#[test]
fn startup_restarts_every_after_close_interval_even_with_a_future_or_snoozed_deadline() {
    for (elapsed, snooze) in [(120, false), (900, false), (120, true), (900, true)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let context = CommandContext::capture(clock.as_ref(), "Создай", None);
        let reminder = app.create(interval(600, None), &context).unwrap();
        clock.advance(600);
        app.tick().unwrap();
        if snooze {
            app.snooze_toast(observed_toast_id(&app, reminder.id), 5)
                .unwrap();
        } else {
            app.close_toast(observed_toast_id(&app, reminder.id))
                .unwrap();
        }
        drop(app);
        clock.advance(elapsed);
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        assert!(
            app.open_toasts().is_empty(),
            "startup must not replay a snoozed after-close toast"
        );
        assert_eq!(
            next(&app, reminder.id),
            Some(clock.sample().utc + Duration::minutes(10))
        );
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(persisted["snoozed"].as_array().unwrap().is_empty());
        assert_eq!(
            persisted["reminders"][0]["schedule"]["next_utc"],
            serde_json::json!(next(&app, reminder.id))
        );
        clock.advance(599);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.advance(1);
        app.tick().unwrap();
        assert_eq!(app.open_toasts()[0].reminder_id, Some(reminder.id));
    }
}

#[test]
fn resuming_an_overdue_paused_after_close_interval_restarts_instead_of_leaving_a_past_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let id = app.create(interval(600, None), &context).unwrap().id;
    app.set_paused(id, true).unwrap();
    clock.advance(900);
    app.tick().unwrap();
    app.set_paused(id, false).unwrap();
    app.tick().unwrap();
    assert_eq!(
        next(&app, id),
        Some(clock.sample().utc + Duration::minutes(10))
    );
    assert!(app.open_toasts().is_empty());
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(id));
}

#[test]
fn a_due_unprocessed_after_close_occurrence_is_reported_as_waiting_for_display() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let context = CommandContext::capture(clock.as_ref(), "Создай", None);
    let id = app.create(interval(600, None), &context).unwrap().id;
    clock.advance(900);
    assert_eq!(
        app.schedule_status(id).as_deref(),
        Some("Ожидает показа уведомления")
    );
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, Some(id));
    assert_eq!(
        app.schedule_status(id).as_deref(),
        Some("Ожидает закрытия уведомления")
    );
}
