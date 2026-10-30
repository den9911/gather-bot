# Gather Bot implementation notes

The initial bot uses Telegram long polling and SQLite. Events are scoped to a chat and require a future RFC 3339 timestamp with an offset and a capacity. Attendance is ordered by join timestamp and user ID; an immediate SQLite transaction serializes joins and departures. Removing a confirmed attendee promotes the earliest waitlisted attendee. Only the creator may cancel an event.

A background task polls every 30 seconds for events starting within one hour and marks reminders after a successful Telegram send. Delivery is at least once around crashes, and no reminder is sent for an event that has already started. Language is stored per chat; English is the default. Event time is stored and displayed in UTC.

Event confirmations include inline Join, Leave and People buttons and accept an optional location. The list is displayed in UTC; confirmation preserves the supplied offset. Automated tests cover waitlist promotion and chat isolation. Live Telegram operation is not yet verified. Named time zones and voting on times are future work. Keep exactly one running instance per token.
