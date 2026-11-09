# Gather Bot implementation notes

The initial bot uses Telegram long polling and SQLite. Events are scoped to a chat and require a future RFC 3339 timestamp with an offset and a capacity. Attendance is ordered by join timestamp and user ID; an immediate SQLite transaction serializes joins and departures. Removing a confirmed attendee promotes the earliest waitlisted attendee. Only the creator may cancel an event.

A background task polls every 30 seconds for events starting within one hour and marks reminders after a successful Telegram send. Delivery is at least once around crashes, and no reminder is sent for an event that has already started. Language is stored per chat; English is the default. Event time is stored and displayed in UTC.

Event confirmations include inline Join, Leave and People buttons and accept an optional location. The list is displayed in UTC; confirmation preserves the supplied offset. Automated tests cover waitlist promotion and chat isolation. Live Telegram operation is not yet verified. Named time zones and voting on times are future work. Keep exactly one running instance per token.


## Command outcome and repeat contract

Command parsing, validation and SQLite business rules live in src/commands.rs. Transport consumes Ignored, Reply or Created with an authoritative event ID/language. INSERT RETURNING id and an immediate transaction replace the old MAX(id)/last_insert_rowid heuristic. Created metadata is returned after commit; validation failures and database errors have no inline keyboard. Calendar export and existing chat/owner isolation remain intact.

New nullable source_message_id/source_fingerprint columns and a unique per-chat source-message index preserve legacy events. SHA-256 of the trimmed create message plus the stored owner binds replay to its original request. Existing matching messages reuse the committed event even after restart; changed data or sender is rejected. Closed/cancelled events are not reopened and receive no Join keyboard on replay. A genuinely new Telegram message is a new request. There is no permanent tombstone after manual database deletion, and duplicate outgoing confirmations are not an exactly-once delivery guarantee.

replies.rs limits UTF-16 output without cutting Unicode scalars, at most four 3500-unit parts, with an explicit truncation note for excessive legacy data. Input commands are capped at 4096 UTF-16 units before parsing; names stored for attendance are capped at 60 characters, callbacks at 64 bytes, popup replies at 190 UTF-16 units. Existing list/people limits remain 20/50. SQLite lock wait is 750 ms. Synchronous SQLite/shared locking remains a future architectural boundary; no unsupported dispatcher queue setting is presented as a global concurrency guarantee.

People callbacks send the full bounded response after a short acknowledgement; unavailable messages do not select another chat. SQL/user text is not copied into command-processing error messages. Tests reproduce the old false-keyboard predicate, trigger-induced insertion and rejection, legacy migration, persisted and concurrent replay, changed owner/payload, cancelled state, EN/RU, calendar/waitlist invariants and Unicode budgets. These are real SQLite/domain tests; real Telegram credentials, physical launchers and transport interruption are separate checks for the remaining W0041 work.

## W0041 recovery and transport verification

Six transport/recovery tests use a local HTTP fake API with a proxy-free, bounded-timeout client and fake credentials. They call the actual message/callback handlers and verify a connection closed after receiving a POST, cancellation while waiting for an API reply, sixteen failed replies followed by success, repair of a corrupted saved fingerprint, and full bounded People output plus an unavailable-message acknowledgement. Database Arc ownership and mutex availability are checked after cancellation/failure. A child test process exits immediately after a real command commit, without running SQLite stack destructors; reopen/replay preserves one event and passes integrity_check. These are not physical power-loss or production-load benchmarks.

Created metadata remains commit-based. SQL rejection produces a generic /list recovery hint with no keyboard or private SQL error; manual repair of invalid metadata re-enables the same original receipt rather than creating a replacement event. The application and handler error callback emit at most five fixed warning categories per process. Framework listener logging, real bot credentials, manual platform launchers and reminder exactly-once behavior are separate operational concerns.
