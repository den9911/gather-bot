# Gather Bot

![Gather Bot illustrated cover](assets/cover.svg)

**English** · [Русский](#русский)

A small Telegram bot for arranging meetups: create an event, gather RSVPs and keep a waitlist. Written in Rust with SQLite.

> Early implementation: the live Telegram interaction still needs a test with a real bot token.

## Setup

1. Install [Rust and Cargo](https://rustup.rs/). Create a bot using [@BotFather](https://t.me/BotFather) and obtain its token.
2. Set `TELOXIDE_TOKEN` in your terminal. On macOS: `export TELOXIDE_TOKEN='your-token'`; on Windows Command Prompt: `set TELOXIDE_TOKEN=your-token`. Do not commit the token.
3. Run `cargo run --release`, or use `run.bat` on Windows or `sh run.command` on macOS in the same terminal. The first run downloads dependencies. The bot uses long polling; run only one instance per token.
4. Open the bot in Telegram or add it to a group. Use commands addressed to the bot (for example, `/help@YourBot`).

`GATHER_DB` optionally sets the SQLite file path; default: `gather.sqlite` in the working directory. Keep this file private and back it up when the bot is stopped. The computer must be running for reminders.

## Commands

- `/create Board games | 2027-01-20T18:00:00+03:00 | 8 | Cafe` — create an event with a future timestamp including a UTC offset and a capacity from 1 to 1000; the place is optional. The event message includes Join, Leave and People buttons.
- `/list`, `/join 1`, `/leave 1`, `/people 1`, `/cancel 1` — list, RSVP, leave, or cancel. Only the organizer can cancel; each chat has its own events.
- `/lang en` or `/lang ru` — change the language for the current chat. `/help` shows the commands.

New event confirmations show the supplied UTC offset; the event list is displayed in UTC. The bot sends one group reminder in the hour before an event while it is running. Reminders may be repeated if it crashes after sending. Waitlist order is based on join time, with user ID as a tie-breaker. Telegram controls its own light and dark appearance.

## Русский

Небольшой Telegram-бот для встреч: создание события, запись участников и очередь ожидания. Rust и SQLite.

> Ранняя версия: работа с настоящим Telegram-ботом пока не проверена.

1. Установите [Rust и Cargo](https://rustup.rs/), создайте бота через [@BotFather](https://t.me/BotFather) и получите токен.
2. В терминале macOS задайте `export TELOXIDE_TOKEN='ваш-токен'`; в командной строке Windows — `set TELOXIDE_TOKEN=ваш-токен`. Не публикуйте токен.
3. В том же терминале выполните `cargo run --release`, либо `sh run.command` на macOS или `run.bat` на Windows. Для первой сборки нужны интернет и зависимости. Запускайте только один экземпляр с этим токеном.
4. Откройте бота в Telegram или добавьте в группу. Используйте команды, адресованные боту: например, `/help@YourBot`.

Пример: `/create Настольные игры | 2027-01-20T18:00:00+03:00 | 8`. Команды `/list`, `/join 1`, `/leave 1`, `/people 1`, `/cancel 1` управляют встречами. Язык чата: `/lang ru` или `/lang en`.

Дата вводится со смещением относительно UTC. Подтверждение показывает это смещение; список встреч — UTC. Файл данных — `gather.sqlite` либо путь из `GATHER_DB`; сохраняйте его копию, когда бот остановлен. Для напоминаний бот должен работать. После сбоя напоминание может повториться. Оформление Telegram определяется самим приложением Telegram.
