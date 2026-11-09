use std::sync::atomic::{AtomicU8, Ordering};

pub enum Diagnostic {
    Command,
    Callback,
    Transport,
    ReminderQuery,
    ReminderUpdate,
}
static REPORTED: AtomicU8 = AtomicU8::new(0);

pub fn report(category: Diagnostic) {
    let (mask, message) = match category {
        Diagnostic::Command => (
            1,
            "Command processing failed; inspect /list before a new create request",
        ),
        Diagnostic::Callback => (2, "Callback processing failed; use a fresh /list message"),
        Diagnostic::Transport => (
            4,
            "Telegram response not confirmed; inspect state before retrying",
        ),
        Diagnostic::ReminderQuery => (8, "Reminder query failed; check the database"),
        Diagnostic::ReminderUpdate => {
            (16, "Reminder confirmation not saved; a reminder may repeat")
        }
    };
    // At most five fixed application warnings per process. Never format request/error payloads.
    if REPORTED.fetch_or(mask, Ordering::Relaxed) & mask == 0 {
        eprintln!("{message}");
    }
}
