use napomni::reminders::{Application, ReminderId, ToastId};

/// Resolve an existing reminder regression fixture to its observed card.
pub fn observed_toast_id(app: &Application, reminder_id: ReminderId) -> ToastId {
    app.open_toasts()
        .iter()
        .find(|toast| toast.reminder_id == Some(reminder_id))
        .expect("expected an open toast for the reminder")
        .id
}
