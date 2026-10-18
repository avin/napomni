//! Mutations operate on saved reminder data; open toast snapshots are independent.
use crate::reminders::{
    Application, CommandContext, Persisted, Reminder, ReminderId, ScheduleInput, quoted_text,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub id: ReminderId,
    pub before: Option<Reminder>,
    pub index: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReminderPatch {
    pub text: Option<String>,
    pub schedule: Option<ScheduleInput>,
    pub border: Option<u32>,
    pub sound: Option<bool>,
}
impl Application {
    pub fn update(
        &mut self,
        id: ReminderId,
        patch: ReminderPatch,
        context: &CommandContext,
    ) -> Result<Reminder> {
        ensure!(
            context.selected_id.is_none_or(|selected| selected == id),
            "Выбрана другая запись. Уточните нужное напоминание"
        );
        ensure!(
            patch.text.is_some()
                || patch.schedule.is_some()
                || patch.border.is_some()
                || patch.sound.is_some(),
            "Укажите свойство для изменения"
        );
        let mut next = self.state.clone();
        let index = next
            .reminders
            .iter()
            .position(|r| r.id == id)
            .context("Напоминание с этим ID не найдено")?;
        let before = next.reminders[index].clone();
        let reminder = next
            .reminders
            .iter_mut()
            .find(|r| r.id == id)
            .context("Напоминание с этим ID не найдено")?;
        if let Some(text) = patch.text {
            let text = quoted_text(&context.source).unwrap_or(text);
            ensure!(!text.trim().is_empty(), "Укажите текст напоминания");
            reminder.text = text;
        }
        if let Some(schedule) = patch.schedule {
            reminder.schedule = schedule.resolve(context, self.clock())?;
            reminder.completed = false;
            reminder.calendar_cursor = None;
        }
        if let Some(border) = patch.border {
            ensure!(border <= 0xffffff, "Цвет должен быть RGB от 0 до 16777215");
            reminder.border = border;
        }
        if let Some(sound) = patch.sound {
            reminder.sound = sound;
        }
        reminder.completed = reminder
            .schedule
            .ended_at(self.clock.sample().utc, self.clock());
        let saved = reminder.clone();
        let schedule_changed = saved.schedule != before.schedule;
        if saved != before {
            self.commit_change(
                next,
                UndoEntry {
                    id,
                    before: Some(before),
                    index,
                },
            )?;
            if schedule_changed {
                self.eligible_after.insert(id, self.clock.sample().utc);
            }
        }
        Ok(saved)
    }
    fn commit_change(&mut self, mut next: Persisted, undo: UndoEntry) -> Result<()> {
        let id = undo.id;
        self.skip_calendar_through_now(&mut next, id);
        next.undo = Some(undo);
        self.store.save(&next)?;
        self.state = next;
        self.skip_through(id, self.clock.sample().utc);
        Ok(())
    }
    fn skip_calendar_through_now(&self, next: &mut Persisted, id: ReminderId) {
        if let Some(reminder) = next.reminders.iter_mut().find(|r| r.id == id)
            && reminder.schedule.is_calendar_repeating()
        {
            let now = self.clock.local_at(self.clock.sample().utc);
            reminder.calendar_cursor = Some(
                reminder
                    .calendar_cursor
                    .map_or(now, |cursor| cursor.max(now)),
            );
        }
    }
    pub fn set_paused(&mut self, id: ReminderId, paused: bool) -> Result<Reminder> {
        let index = self
            .state
            .reminders
            .iter()
            .position(|r| r.id == id)
            .context("Напоминание с этим ID не найдено")?;
        let before = self.state.reminders[index].clone();
        if before.paused == paused {
            return Ok(before);
        }
        let mut next = self.state.clone();
        next.reminders[index].paused = paused;
        next.reminders[index].completed = next.reminders[index]
            .schedule
            .ended_at(self.clock.sample().utc, self.clock());
        let saved = next.reminders[index].clone();
        self.commit_change(
            next,
            UndoEntry {
                id,
                before: Some(before),
                index,
            },
        )?;
        Ok(saved)
    }
    pub fn delete(&mut self, id: ReminderId) -> Result<Reminder> {
        let index = self
            .state
            .reminders
            .iter()
            .position(|r| r.id == id)
            .context("Напоминание с этим ID не найдено")?;
        let mut next = self.state.clone();
        let before = next.reminders.remove(index);
        self.commit_change(
            next,
            UndoEntry {
                id,
                before: Some(before.clone()),
                index,
            },
        )?;
        Ok(before)
    }
    pub fn can_undo(&self) -> bool {
        self.state.undo.is_some()
    }
    pub fn undo(&mut self) -> Result<Option<Reminder>> {
        let undo = self.state.undo.clone().context("Нет операции для отмены")?;
        let mut next = self.state.clone();
        let existing = next.reminders.iter().position(|r| r.id == undo.id);
        let restored = undo.before.map(|mut reminder| {
            if let Some(index) = existing {
                let current = &next.reminders[index];
                if current.schedule == reminder.schedule {
                    // Preserve scheduling progress accrued since the data operation.
                    let mut merged = current.clone();
                    merged.text = reminder.text;
                    merged.border = reminder.border;
                    merged.sound = reminder.sound;
                    merged.paused = reminder.paused;
                    reminder = merged;
                }
            }
            reminder.completed = reminder
                .schedule
                .ended_at(self.clock.sample().utc, self.clock());
            if let Some(index) = existing {
                next.reminders[index] = reminder.clone();
            } else {
                next.reminders
                    .insert(undo.index.min(next.reminders.len()), reminder.clone());
            }
            reminder
        });
        if restored.is_none() {
            next.reminders.retain(|r| r.id != undo.id);
        }
        next.undo = None;
        self.skip_calendar_through_now(&mut next, undo.id);
        self.store.save(&next)?;
        self.state = next;
        self.skip_through(undo.id, self.clock.sample().utc);
        Ok(restored)
    }
}
