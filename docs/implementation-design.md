# Gather Bot implementation notes

The initial bot uses Telegram long polling and SQLite. Events are scoped to a chat and require a future RFC 3339 timestamp with an offset and a capacity. Attendance is ordered by join timestamp and user ID; an immediate SQLite transaction serializes joins and departures. Removing a confirmed attendee promotes the earliest waitlisted attendee. Only the creator may cancel an event.

A background task polls every 30 seconds for events starting within one hour and marks reminders after a successful Telegram send. Delivery is at least once around crashes, and no reminder is sent for an event that has already started. Language is stored per chat; English is the default. Event time is stored and displayed in UTC.

This is an unverified first implementation. Planned improvements include inline buttons, time zone names and localized displays, automated tests and cross-platform CI. There is no location field or voting on times yet. Keep exactly one running instance per token.
