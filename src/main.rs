mod calendar;
mod commands;
mod replies;
mod startup;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use commands::{execute, execute_message, language, words};
use rusqlite::{params, Connection};
use std::{
    env,
    sync::{Arc, Mutex},
    time::Duration,
};
use teloxide::{
    prelude::*,
    types::{CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message},
};

type Db = Arc<Mutex<Connection>>;

fn init_db(path: &str) -> Result<Db> {
    let db = Connection::open(path).context("opening database")?;
    db.busy_timeout(Duration::from_millis(750))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
      CREATE TABLE IF NOT EXISTS chats(chat_id INTEGER PRIMARY KEY, lang TEXT NOT NULL DEFAULT 'en');
      CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, chat_id INTEGER NOT NULL, owner INTEGER NOT NULL, title TEXT NOT NULL, starts INTEGER NOT NULL, capacity INTEGER NOT NULL, cancelled INTEGER NOT NULL DEFAULT 0, reminded INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS attendance(event_id INTEGER NOT NULL REFERENCES events(id), user_id INTEGER NOT NULL, name TEXT NOT NULL, joined INTEGER NOT NULL, PRIMARY KEY(event_id,user_id));")?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS app_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL); INSERT OR IGNORE INTO app_meta(key,value) VALUES('instance',lower(hex(randomblob(16))))")?;
    for (column, declaration) in [
        ("location", "TEXT NOT NULL DEFAULT ''"),
        ("display_time", "TEXT NOT NULL DEFAULT ''"),
        ("source_message_id", "INTEGER"),
        ("source_fingerprint", "BLOB"),
    ] {
        let found = {
            let mut statement = db.prepare("PRAGMA table_info(events)")?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            names.iter().any(|name| name == column)
        };
        if !found {
            db.execute(
                &format!("ALTER TABLE events ADD COLUMN {column} {declaration}"),
                [],
            )?;
        }
    }
    db.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS events_source_message ON events(chat_id,source_message_id) WHERE source_message_id IS NOT NULL")?;
    Ok(Arc::new(Mutex::new(db)))
}
async fn reminders(bot: Bot, db: Db) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let pending: Vec<(i64,i64,String)> = match db.lock() {
            Ok(conn) => match conn.prepare("SELECT id,chat_id,title FROM events WHERE cancelled=0 AND reminded=0 AND starts>?1 AND starts<=?2 LIMIT 50") {
                Ok(mut stmt)=>stmt.query_map(params![Utc::now().timestamp(),Utc::now().timestamp()+3600],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).ok().map(|rows|rows.filter_map(|r|r.ok()).collect()).unwrap_or_default(),
                Err(e)=>{eprintln!("reminder query: {e}");Vec::new()}
            }, Err(_)=>Vec::new()
        };
        for (id, chat, title) in pending {
            let ru = db
                .lock()
                .ok()
                .and_then(|c| language(&c, chat).ok())
                .as_deref()
                == Some("ru");
            let msg = format!(
                "{} #{}: {}",
                words(ru, "Starting within one hour", "Начнётся в течение часа"),
                id,
                title
            );
            if bot.send_message(ChatId(chat), msg).await.is_ok() {
                if let Ok(conn) = db.lock() {
                    if let Err(e) = conn.execute("UPDATE events SET reminded=1 WHERE id=?1", [id]) {
                        eprintln!("reminder update: {e}");
                    }
                }
            }
        }
    }
}
fn keyboard(id: i64, ru: bool) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(words(ru, "✓ Join", "✓ Пойду"), format!("join:{id}")),
        InlineKeyboardButton::callback(words(ru, "Leave", "Не смогу"), format!("leave:{id}")),
        InlineKeyboardButton::callback(words(ru, "People", "Участники"), format!("people:{id}")),
    ]])
}
async fn message_handler(bot: Bot, msg: Message, db: Db) -> ResponseResult<()> {
    let Some(text) = msg.text() else {
        return respond(());
    };
    let Some(sender) = msg.from.as_ref() else {
        return respond(());
    };
    if !commands::valid_input(text) {
        bot.send_message(msg.chat.id, "Command is too long / Слишком длинная команда")
            .await?;
        return respond(());
    }
    if text
        .split_whitespace()
        .next()
        .unwrap_or("")
        .split('@')
        .next()
        == Some("/calendar")
    {
        let id = text
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|n| *n > 0);
        let result = match id {
            Some(id) => match db.lock() {
                Ok(conn) => calendar::export(&conn, msg.chat.id.0, id),
                Err(_) => Err(anyhow!("database lock poisoned")),
            },
            None => Ok(None),
        };
        match result {
            Ok(Some(bytes)) => {
                bot.send_document(
                    msg.chat.id,
                    teloxide::types::InputFile::memory(bytes).file_name("gather-event.ics"),
                )
                .caption("Calendar snapshot, not a subscription / Снимок события, не подписка")
                .await?;
            }
            _ => {
                bot.send_message(msg.chat.id,"Event unavailable in this chat. Use /calendar ID. / Встреча недоступна в этом чате. /calendar ID").await?;
            }
        }
        return respond(());
    }
    let chat = msg.chat.id.0;
    let user = match i64::try_from(sender.id.0) {
        Ok(id) if id > 0 => id,
        _ => return respond(()),
    };
    let response = match db.lock() {
        Ok(mut conn) => execute_message(
            &mut conn,
            chat,
            user,
            &sender.full_name(),
            text,
            Some(msg.id.0),
        ),
        Err(_) => Err(anyhow!("database lock poisoned")),
    };
    match response {
        Ok(outcome) => {
            let created = outcome.created();
            for (index, part) in replies::parts(outcome.text()).into_iter().enumerate() {
                let request = bot.send_message(msg.chat.id, part);
                if let Some((id, ru)) = created.filter(|_| index == 0) {
                    request.reply_markup(keyboard(id, ru)).await?;
                } else {
                    request.await?;
                }
            }
        }
        Err(_) => {
            eprintln!("Command processing failed");
            bot.send_message(
                msg.chat.id,
                "Could not process this request. / Не удалось выполнить запрос.",
            )
            .await?;
        }
    }
    respond(())
}
async fn callback_handler(bot: Bot, q: CallbackQuery, db: Db) -> ResponseResult<()> {
    let mut full_reply = None;
    let answer = if let (Some(message), Some(data)) = (q.regular_message(), q.data.as_deref()) {
        if let (Some((action, id)), Ok(user)) =
            (replies::callback(data), i64::try_from(q.from.id.0))
        {
            if user <= 0 {
                "Sender unavailable / Отправитель недоступен".into()
            } else {
                let response = match db.lock() {
                    Ok(mut conn) => execute(
                        &mut conn,
                        message.chat.id.0,
                        user,
                        &q.from.full_name(),
                        &format!("/{action} {id}"),
                    ),
                    Err(_) => Err(anyhow!("database lock poisoned")),
                };
                match response {
                    Ok(outcome) if action == "people" => {
                        full_reply = Some((message.chat.id, outcome.text().to_owned()));
                        "Participants / Участники".into()
                    }
                    Ok(outcome) => outcome.text().to_owned(),
                    Err(_) => {
                        eprintln!("Callback processing failed");
                        "Request failed / Ошибка запроса".into()
                    }
                }
            }
        } else {
            "Invalid action / Неверное действие".into()
        }
    } else {
        "Message unavailable / Сообщение недоступно".into()
    };
    bot.answer_callback_query(q.id)
        .text(replies::short(&answer))
        .show_alert(full_reply.is_none())
        .await?;
    if let Some((chat, text)) = full_reply {
        for part in replies::parts(&text) {
            bot.send_message(chat, part).await?;
        }
    }
    respond(())
}
#[tokio::main]
async fn main() -> Result<()> {
    if std::env::args().any(|a| a == "--check") {
        anyhow::ensure!(
            env::var("TELOXIDE_TOKEN").is_ok_and(|v| !v.trim().is_empty()),
            "Set TELOXIDE_TOKEN / Задайте TELOXIDE_TOKEN"
        );
        return startup::check(
            Some(std::path::Path::new(
                &env::var("GATHER_DB").unwrap_or_else(|_| "gather.sqlite".into()),
            )),
            None,
        );
    }
    let token =
        env::var("TELOXIDE_TOKEN").context("Set TELOXIDE_TOKEN to your Telegram bot token")?;
    let db = init_db(&env::var("GATHER_DB").unwrap_or_else(|_| "gather.sqlite".into()))?;
    let bot = Bot::new(token);
    tokio::spawn(reminders(bot.clone(), db.clone()));
    let handler = teloxide::dptree::entry()
        .branch(Update::filter_message().endpoint(message_handler))
        .branch(Update::filter_callback_query().endpoint(callback_handler));
    Dispatcher::builder(bot, handler)
        .dependencies(teloxide::dptree::deps![db])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn meetup_waitlist_and_chat_scope() -> Result<()> {
        let db = init_db(":memory:")?;
        let mut db = db.lock().map_err(|_| anyhow!("lock"))?;
        let date = (Utc::now() + chrono::Duration::days(2)).to_rfc3339();
        let text = format!("/create Dinner | {date} | 1 | Cafe");
        let created = execute(&mut db, 1, 11, "Alice", &text)?;
        let id = created.created().context("created metadata")?.0;
        assert!(execute(&mut db, 1, 11, "Alice", &format!("/join {id}"))?
            .text()
            .contains("attending"));
        assert!(execute(&mut db, 1, 22, "Bob", &format!("/join {id}"))?
            .text()
            .contains("Waitlist"));
        assert!(execute(&mut db, 2, 22, "Bob", &format!("/join {id}"))?
            .text()
            .contains("not found"));
        assert!(execute(&mut db, 1, 11, "Alice", &format!("/leave {id}"))?
            .text()
            .contains("Bob"));
        assert!(execute(&mut db, 1, 22, "Bob", &format!("/cancel {id}"))?
            .text()
            .contains("organizer"));
        execute(&mut db, 1, 11, "Alice", &format!("/cancel {id}"))?;
        assert!(execute(&mut db, 1, 22, "Bob", &format!("/join {id}"))?
            .text()
            .contains("not found"));
        Ok(())
    }
}

#[cfg(test)]
mod calendar_tests {
    use super::*;
    #[test]
    fn calendar_scopes_cancel_and_stable_uid() -> Result<()> {
        let db = init_db(":memory:")?;
        let mut c = db.lock().map_err(|_| anyhow!("lock"))?;
        let date = (Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        let created = execute(
            &mut c,
            1,
            5,
            "Name",
            &format!("/create Привет, мир | {date} | 2 | Cafe"),
        )?;
        let id = created.created().context("created metadata")?.0;
        let a = calendar::export(&c, 1, id)?.context("file")?;
        let b = calendar::export(&c, 1, id)?.context("file")?;
        let uid = |v: Vec<u8>| {
            String::from_utf8_lossy(&v)
                .lines()
                .find(|l| l.starts_with("UID:"))
                .unwrap_or("")
                .to_owned()
        };
        assert_eq!(uid(a), uid(b));
        assert!(calendar::export(&c, 2, id)?.is_none());
        execute(&mut c, 1, 5, "Name", &format!("/cancel {id}"))?;
        assert!(calendar::export(&c, 1, id)?.is_none());
        Ok(())
    }
}
