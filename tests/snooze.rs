use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use napomni::{
    mutations::ReminderPatch,
    reminders::{
        Application, Clock, CommandContext, CreateReminder, JsonStore, Persisted, ReminderStore,
        ScheduleInput, TimeSample,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

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
        utc.naive_utc()
    }

    fn resolve_local(&self, local: NaiveDateTime) -> Vec<DateTime<Utc>> {
        vec![local.and_utc()]
    }
}

fn create(app: &mut Application, schedule: ScheduleInput) -> u64 {
    app.create(
        CreateReminder {
            text: "Размяться".into(),
            schedule,
            border: Some(0x123456),
            sound: Some(true),
        },
        &CommandContext::capture(app.clock(), "Создай", None),
    )
    .unwrap()
    .id
}

fn fixed(seconds: i64) -> ScheduleInput {
    ScheduleInput::Interval {
        seconds,
        start_local: None,
    }
}

fn after_close() -> ScheduleInput {
    ScheduleInput::IntervalAfterClose {
        seconds: 600,
        start_local: None,
    }
}

#[test]
fn completed_once_can_be_snoozed_repeatedly_and_preserves_its_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, ScheduleInput::AfterSeconds { seconds: 60 });
    clock.advance(60);
    app.tick().unwrap();
    assert!(app.reminders()[0].completed);
    let mut snapshot = app.open_toasts()[0].clone();
    app.update(
        id,
        ReminderPatch {
            text: Some("Изменённый текст".into()),
            sound: Some(false),
            border: Some(0xabcdef),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Измени", Some(id)),
    )
    .unwrap();
    for _ in 0..2 {
        app.snooze_toast(id, 5).unwrap();
        assert!(app.open_toasts().is_empty());
        assert!(!app.signal_active());
        clock.advance(299);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.advance(1);
        app.tick().unwrap();
        let shown = &app.open_toasts()[0];
        assert!(shown.sequence > snapshot.sequence);
        snapshot.sequence = shown.sequence;
        assert_eq!(shown, &snapshot);
        assert!(app.signal_active());
    }
}

#[test]
fn snoozing_after_close_interval_does_not_start_the_next_interval() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, after_close());
    clock.advance(600);
    app.tick().unwrap();
    app.close_toast(id).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.advance(300);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    clock.advance(900);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    app.close_toast(id).unwrap();
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn fixed_occurrence_at_or_before_snooze_cancels_the_old_snooze() {
    for period in [180, 300] {
        let directory = tempfile::tempdir().unwrap();
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(
            clock.clone(),
            Box::new(JsonStore::new(directory.path().join("data.json"))),
        )
        .unwrap();
        let id = create(&mut app, fixed(period));
        clock.advance(period);
        app.tick().unwrap();
        app.snooze_toast(id, 5).unwrap();
        clock.advance(period);
        app.tick().unwrap();
        assert_eq!(app.open_toasts().len(), 1);
        app.close_toast(id).unwrap();
        if period < 300 {
            clock.advance(300 - period);
            app.tick().unwrap();
            assert!(app.open_toasts().is_empty());
        }
        // A second tick at the coinciding deadline must not reveal another toast.
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
    }
}

#[test]
fn snooze_before_fixed_occurrence_keeps_the_original_phase_and_one_open_toast() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, fixed(600));
    clock.advance(600);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.advance(300);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    clock.advance(300);
    app.tick().unwrap();
    assert_eq!(app.open_toasts(), &[snapshot]);
    app.close_toast(id).unwrap();
    clock.advance(599);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn closing_snoozed_fixed_toast_does_not_shift_the_next_regular_occurrence() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, fixed(600));
    clock.advance(600);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.advance(300);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
    app.close_toast(id).unwrap();
    clock.advance(299);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn edits_while_snoozed_do_not_change_pending_snapshot_after_restart() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("data.json");
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    let id = create(&mut app, fixed(600));
    clock.advance(600);
    app.tick().unwrap();
    let original = app.open_toasts()[0].clone();
    app.snooze_toast(id, 5).unwrap();
    app.update(
        id,
        ReminderPatch {
            text: Some("Новый текст".into()),
            sound: Some(false),
            border: Some(0xabcdef),
            ..Default::default()
        },
        &CommandContext::capture(clock.as_ref(), "Измени", Some(id)),
    )
    .unwrap();
    drop(app);
    let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
    clock.advance(300);
    app.tick().unwrap();
    let shown = &app.open_toasts()[0];
    assert_eq!(shown.text, original.text);
    assert_eq!(shown.border, original.border);
    assert_eq!(shown.sound, original.sound);
    app.close_toast(id).unwrap();
    clock.advance(600);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].text, "Новый текст");
    assert!(!app.signal_active());
}

#[test]
fn snoozing_one_toast_keeps_aggregate_sound_until_the_last_sounding_toast_is_hidden() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let first = create(&mut app, after_close());
    let second = create(&mut app, after_close());
    clock.advance(600);
    app.tick().unwrap();
    app.snooze_toast(first, 5).unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    assert!(app.signal_active());
    app.snooze_toast(second, 5).unwrap();
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
    clock.advance(300);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 2);
    assert!(app.signal_active());
}

#[test]
fn pending_snooze_survives_restart_for_ordinary_schedules() {
    for schedule in [ScheduleInput::AfterSeconds { seconds: 600 }, fixed(600)] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let id = create(&mut app, schedule);
        clock.advance(600);
        app.tick().unwrap();
        let original = app.open_toasts()[0].clone();
        app.snooze_toast(id, 5).unwrap();
        drop(app);
        clock.advance(120);
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        assert!(app.open_toasts().is_empty());
        clock.advance(179);
        app.tick().unwrap();
        assert!(app.open_toasts().is_empty());
        clock.advance(1);
        app.tick().unwrap();
        assert_eq!(app.open_toasts()[0].reminder_id, id);
        assert_eq!(app.open_toasts()[0].text, original.text);
        app.close_toast(id).unwrap();
        drop(app);
        let app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        assert!(app.open_toasts().is_empty());
    }
}

#[test]
fn expired_after_close_snooze_reappears_after_sleep_but_restarts_after_app_restart() {
    for restart in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("data.json");
        let clock = Arc::new(TestClock::new());
        let mut app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        let id = create(&mut app, after_close());
        clock.advance(600);
        app.tick().unwrap();
        app.snooze_toast(id, 5).unwrap();
        clock.sleep(900);
        if restart {
            drop(app);
            app = Application::open(clock.clone(), Box::new(JsonStore::new(&path))).unwrap();
        } else {
            app.tick().unwrap();
        }
        if restart {
            assert!(app.open_toasts().is_empty());
            clock.advance(599);
            app.tick().unwrap();
            assert!(app.open_toasts().is_empty());
            clock.advance(1);
            app.tick().unwrap();
        }
        assert_eq!(app.open_toasts().len(), 1);
        assert_eq!(app.open_toasts()[0].reminder_id, id);
    }
}

#[test]
fn fixed_occurrence_during_sleep_supersedes_the_pending_snooze() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, fixed(180));
    clock.advance(180);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.sleep(301);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(58);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn pause_and_delete_cancel_pending_snooze_and_reject_snoozing_stale_toasts() {
    for delete in [false, true] {
        for snoozed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let clock = Arc::new(TestClock::new());
            let mut app = Application::open(
                clock.clone(),
                Box::new(JsonStore::new(directory.path().join("data.json"))),
            )
            .unwrap();
            let id = create(&mut app, after_close());
            clock.advance(600);
            app.tick().unwrap();
            if snoozed {
                app.snooze_toast(id, 5).unwrap();
            }
            if delete {
                app.delete(id).unwrap();
            } else {
                app.set_paused(id, true).unwrap();
            }
            if !snoozed {
                assert!(app.snooze_toast(id, 5).is_err());
                assert_eq!(app.open_toasts().len(), 1);
                app.close_toast(id).unwrap();
            }
            clock.advance(300);
            app.tick().unwrap();
            assert!(app.open_toasts().is_empty());
        }
    }
}

#[test]
fn invalid_delay_does_not_dismiss_the_toast() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, after_close());
    clock.advance(600);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    assert!(app.snooze_toast(id, 0).is_err());
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert!(app.signal_active());
}

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

#[test]
fn failed_snooze_save_keeps_toast_and_sound_and_retry_uses_actual_snooze_time() {
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
    let id = create(&mut app, after_close());
    clock.advance(600);
    app.tick().unwrap();
    let snapshot = app.open_toasts()[0].clone();
    fail.store(true, Ordering::SeqCst);
    assert!(app.snooze_toast(id, 5).is_err());
    assert_eq!(app.open_toasts(), &[snapshot]);
    assert!(app.signal_active());
    clock.advance(120);
    fail.store(false, Ordering::SeqCst);
    app.snooze_toast(id, 5).unwrap();
    clock.advance(299);
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
    clock.advance(1);
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}

#[test]
fn failed_redisplay_save_preserves_pending_snooze_for_retry_without_duplicate() {
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
    let id = create(&mut app, ScheduleInput::AfterSeconds { seconds: 60 });
    clock.advance(60);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.advance(300);
    fail.store(true, Ordering::SeqCst);
    assert!(app.tick().is_err());
    assert!(app.open_toasts().is_empty());
    assert!(!app.signal_active());
    fail.store(false, Ordering::SeqCst);
    app.tick().unwrap();
    assert_eq!(app.open_toasts().len(), 1);
    app.close_toast(id).unwrap();
    app.tick().unwrap();
    assert!(app.open_toasts().is_empty());
}

#[test]
fn overdue_snooze_never_advertises_a_past_time_while_waiting_for_a_tick() {
    let directory = tempfile::tempdir().unwrap();
    let clock = Arc::new(TestClock::new());
    let mut app = Application::open(
        clock.clone(),
        Box::new(JsonStore::new(directory.path().join("data.json"))),
    )
    .unwrap();
    let id = create(&mut app, after_close());
    clock.advance(600);
    app.tick().unwrap();
    app.snooze_toast(id, 5).unwrap();
    clock.advance(900);
    assert_eq!(
        app.schedule_status(id).as_deref(),
        Some("Ожидает показа уведомления")
    );
    app.tick().unwrap();
    assert_eq!(app.open_toasts()[0].reminder_id, id);
}
