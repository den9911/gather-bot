# Gather Bot: implementation design

Status: design only. The bot has not been implemented yet.

## User flow

1. An organizer adds the bot to a group and creates an event: title, date and time with an explicit IANA time zone, place or link, and optional capacity.
2. The bot posts an event card with Join and Leave buttons. Responses are scoped to the originating chat and event. Joining twice is idempotent.
3. When capacity is full, new participants join a FIFO waitlist. When a confirmed participant leaves, the first person waiting is promoted in the same database transaction.
4. An organizer can view or cancel their event; cancellation prevents subsequent joins and reminders. Event times are stored in UTC and displayed with the event time zone.
5. A background worker sends a group reminder before the event. Reminder status survives restarts; failures are logged and retried with a limit. Telegram may deliver a duplicate if a process crashes after sending but before recording success.

## Proposed implementation

- Rust with Tokio and teloxide, SQLite for persistence and transactions, and a time-zone-aware date library. Prefer typed commands and callback payloads.
- English by default and Russian as an explicit per-chat setting. Telegram controls use inline buttons; Telegram itself controls its theme.
- Long polling for local use, one active instance per bot token. Store token in an environment variable or an ignored local configuration file, never in the repository.
- Tables for events, participants, chat preferences, and reminder jobs. Validate event ownership, chat membership, capacities and callback identifiers server-side. Limit input lengths and handle outdated or duplicate callbacks.
- Use UTC instants for scheduling, and handle ambiguous or nonexistent local times by asking the organizer to choose a valid time or offset.
- Tests cover full capacity, simultaneous join/leave, waitlist promotion, cancellation, reminder retries, restart recovery, daylight-saving transitions and both languages.

## Setup and presentation

- Document BotFather setup, group permissions, storage location, environment variables, backup, and the behavior when the process is stopped.
- Add Windows `run.bat` and macOS `run.command` launchers that check prerequisites, build and run the bot. A running computer or server is needed for reminders.
- Provide an English README with a Russian translation and screenshots captured from the finished bot. Preserve the existing license and gitignore; add secret and database exclusions if necessary.
- CI should compile and test on Windows and macOS before a release is described as working.
