use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::{env, sync::{Arc, Mutex}, time::Duration};
use teloxide::{prelude::*, types::{Message,CallbackQuery,InlineKeyboardButton,InlineKeyboardMarkup}};

type Db = Arc<Mutex<Connection>>;

fn init_db(path: &str) -> Result<Db> {
    let db = Connection::open(path).context("opening database")?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
      CREATE TABLE IF NOT EXISTS chats(chat_id INTEGER PRIMARY KEY, lang TEXT NOT NULL DEFAULT 'en');
      CREATE TABLE IF NOT EXISTS events(id INTEGER PRIMARY KEY, chat_id INTEGER NOT NULL, owner INTEGER NOT NULL, title TEXT NOT NULL, starts INTEGER NOT NULL, capacity INTEGER NOT NULL, cancelled INTEGER NOT NULL DEFAULT 0, reminded INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS attendance(event_id INTEGER NOT NULL REFERENCES events(id), user_id INTEGER NOT NULL, name TEXT NOT NULL, joined INTEGER NOT NULL, PRIMARY KEY(event_id,user_id));")?;
    for (column, declaration) in [("location", "TEXT NOT NULL DEFAULT ''"), ("display_time", "TEXT NOT NULL DEFAULT ''")] {
        let found = { let mut statement=db.prepare("PRAGMA table_info(events)")?; let names=statement.query_map([], |row|row.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?; names.iter().any(|name|name==column) };
        if !found { db.execute(&format!("ALTER TABLE events ADD COLUMN {column} {declaration}"), [])?; }
    }
    Ok(Arc::new(Mutex::new(db)))
}
fn language(db: &Connection, chat: i64) -> Result<String> {
    Ok(db.query_row("SELECT lang FROM chats WHERE chat_id=?1", [chat], |r| r.get(0)).optional()?.unwrap_or_else(|| "en".into()))
}
fn words(ru: bool, en: &'static str, russian: &'static str) -> &'static str { if ru { russian } else { en } }
fn execute(db: &mut Connection, chat: i64, user: i64, name: &str, text: &str) -> Result<String> {
    let ru = language(db, chat)? == "ru";
    let mut parts = text.trim().splitn(2, ' ');
    let command = parts.next().unwrap_or("").split('@').next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();
    match command {
        "/start" | "/help" => Ok(words(ru,
            "Gather Bot\n/create title | 2027-01-20T18:00:00+03:00 | capacity | place (optional)\n/list — upcoming events\n/join ID · /leave ID · /people ID · /cancel ID\n/lang en or /lang ru\nThe date must include an explicit UTC offset. The bot must be running for reminders.",
            "Gather Bot\n/create название | 2027-01-20T18:00:00+03:00 | мест | место (необязательно)\n/list — будущие встречи\n/join ID · /leave ID · /people ID · /cancel ID\n/lang en или /lang ru\nУкажите дату с часовым смещением. Для напоминаний бот должен быть запущен.").into()),
        "/lang" => {
            if !["en", "ru"].contains(&args) { return Ok("Use /lang en or /lang ru".into()); }
            db.execute("INSERT INTO chats(chat_id,lang) VALUES(?1,?2) ON CONFLICT(chat_id) DO UPDATE SET lang=excluded.lang",params![chat,args])?;
            Ok(if args == "ru" { "Язык: русский" } else { "Language: English" }.into())
        }
        "/create" => {
            let fields: Vec<_> = args.split('|').map(str::trim).collect();
            if !(3..=4).contains(&fields.len()) || fields[0].is_empty() || fields[0].chars().count()>120 || fields.get(3).is_some_and(|s|s.chars().count()>200) { return Ok(words(ru,"Use /create title | date with offset | capacity","Формат: /create название | дата со смещением | количество мест").into()); }
            let date = match DateTime::parse_from_rfc3339(fields[1]) { Ok(d) if d.timestamp()>Utc::now().timestamp()=>d, _=>return Ok(words(ru,"Enter a future RFC 3339 date with offset, e.g. 2027-01-20T18:00:00+03:00","Укажите будущую дату со смещением: 2027-01-20T18:00:00+03:00").into()) };
            let capacity: i64 = match fields[2].parse() { Ok(n) if (1..=1000).contains(&n)=>n, _=>return Ok(words(ru,"Capacity must be between 1 and 1000.","Количество мест: от 1 до 1000.").into()) };
            let location=fields.get(3).copied().unwrap_or("");
            db.execute("INSERT INTO events(chat_id,owner,title,starts,capacity,location,display_time) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![chat,user,fields[0],date.timestamp(),capacity,location,date.to_rfc3339()])?;
            Ok(format!("{} #{}: {}\n{}{}\n/join {} · /people {}",words(ru,"Created","Создана встреча"),db.last_insert_rowid(),fields[0],date.format("%Y-%m-%d %H:%M %:z"),if location.is_empty(){String::new()}else{format!("\n📍 {location}")},db.last_insert_rowid(),db.last_insert_rowid()))
        }
        "/list" => {
            let mut statement=db.prepare("SELECT id,title,starts,capacity,(SELECT COUNT(*) FROM attendance a WHERE a.event_id=e.id) FROM events e WHERE chat_id=?1 AND cancelled=0 AND starts>?2 ORDER BY starts LIMIT 20")?;
            let rows=statement.query_map(params![chat,Utc::now().timestamp()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?)))?;
            let mut result=String::new();
            for row in rows { let (id,title,starts,cap,count)=row?; let date=DateTime::from_timestamp(starts,0).map(|d|d.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_default(); result.push_str(&format!("#{id} {title}\n{date} · {}/{} (+{} waitlist)\n/join {id} · /leave {id}\n\n", count.min(cap), cap, (count-cap).max(0))); }
            if result.is_empty(){result=words(ru,"No upcoming events.","Предстоящих встреч нет.").into();} Ok(result)
        }
        "/people" => {
            let id:i64=match args.parse(){Ok(n) if n>0=>n,_=>return Ok(words(ru,"Enter an event ID.","Укажите номер встречи.").into())};
            let event:Option<(String,i64)>=db.query_row("SELECT title,capacity FROM events WHERE id=?1 AND chat_id=?2 AND cancelled=0",params![id,chat],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let Some((title,capacity))=event else{return Ok(words(ru,"Event not found in this chat.","Встреча в этом чате не найдена.").into())};
            let mut statement=db.prepare("SELECT name FROM attendance WHERE event_id=?1 ORDER BY joined,user_id LIMIT 50")?;
            let names=statement.query_map([id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut output=format!("#{} {}\n",id,title);
            for (i,name) in names.iter().enumerate(){output.push_str(&format!("{} {}\n",if (i as i64)<capacity{"✓"}else{"⏳"},name.chars().take(60).collect::<String>()));}
            if names.is_empty(){output.push_str(words(ru,"No participants yet.","Пока никто не записался."));} Ok(output)
        }
        "/join" | "/leave" | "/cancel" => {
            let id:i64=match args.parse(){Ok(n) if n>0=>n,_=>return Ok(words(ru,"Enter an event ID.","Укажите номер встречи.").into())};
            let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let event:Option<(i64,i64,i64)>=tx.query_row("SELECT owner,capacity,starts FROM events WHERE id=?1 AND chat_id=?2 AND cancelled=0",params![id,chat],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let Some((owner,capacity,starts))=event else { return Ok(words(ru,"Event not found in this chat.","Встреча в этом чате не найдена.").into()); };
            if starts<=Utc::now().timestamp(){return Ok(words(ru,"This event has already started.","Эта встреча уже началась.").into());}
            let reply=if command=="/cancel" {
                if owner!=user{return Ok(words(ru,"Only the organizer can cancel.","Отменить встречу может только организатор.").into());}
                tx.execute("UPDATE events SET cancelled=1 WHERE id=?1",[id])?; words(ru,"Event cancelled.","Встреча отменена.").to_string()
            } else if command=="/join" {
                tx.execute("INSERT OR IGNORE INTO attendance(event_id,user_id,name,joined) VALUES(?1,?2,?3,?4)",params![id,user,name,Utc::now().timestamp_millis()])?;
                let position:i64=tx.query_row("SELECT COUNT(*) FROM attendance WHERE event_id=?1 AND (joined < (SELECT joined FROM attendance WHERE event_id=?1 AND user_id=?2) OR (joined = (SELECT joined FROM attendance WHERE event_id=?1 AND user_id=?2) AND user_id<=?2))",params![id,user],|r|r.get(0))?;
                if position<=capacity {words(ru,"You are attending.","Вы записаны на встречу.").to_string()} else {format!("{} #{}",words(ru,"Waitlist position","Место в очереди"),position-capacity)}
            } else {
                let position: Option<i64> = tx.query_row("SELECT (SELECT COUNT(*) FROM attendance b WHERE b.event_id=a.event_id AND (b.joined<a.joined OR (b.joined=a.joined AND b.user_id<=a.user_id))) FROM attendance a WHERE a.event_id=?1 AND a.user_id=?2",params![id,user],|r|r.get(0)).optional()?;
                let count:i64=tx.query_row("SELECT COUNT(*) FROM attendance WHERE event_id=?1",[id],|r|r.get(0))?;
                let changed=tx.execute("DELETE FROM attendance WHERE event_id=?1 AND user_id=?2",params![id,user])?;
                if changed==0 {words(ru,"You were not on the list.","Вас не было в списке.").to_string()} else {
                    let promoted:Option<String>=if position.is_some_and(|n| n<=capacity) && count>capacity { tx.query_row("SELECT name FROM attendance WHERE event_id=?1 ORDER BY joined,user_id LIMIT 1 OFFSET ?2",params![id,capacity-1],|r|r.get(0)).optional()? } else { None };
                    match promoted { Some(n)=>format!("{} {n}",words(ru,"Updated attendance. The next attendee is:","Список обновлён. Следующий участник:")), None=>words(ru,"You have left the event.","Вы вышли из встречи.").to_string() }
                }
            }; tx.commit()?; Ok(reply)
        }
        _ => Ok(String::new()),
    }
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
        for (id,chat,title) in pending {
            let ru=db.lock().ok().and_then(|c|language(&c,chat).ok()).as_deref()==Some("ru");
            let msg=format!("{} #{}: {}",words(ru,"Starting within one hour","Начнётся в течение часа"),id,title);
            if bot.send_message(ChatId(chat),msg).await.is_ok() {if let Ok(conn)=db.lock(){if let Err(e)=conn.execute("UPDATE events SET reminded=1 WHERE id=?1",[id]){eprintln!("reminder update: {e}");}}}
        }
    }
}
fn keyboard(id:i64,ru:bool)->InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![
        InlineKeyboardButton::callback(words(ru,"✓ Join","✓ Пойду"),format!("join:{id}")),
        InlineKeyboardButton::callback(words(ru,"Leave","Не смогу"),format!("leave:{id}")),
        InlineKeyboardButton::callback(words(ru,"People","Участники"),format!("people:{id}"))
    ]])
}
async fn message_handler(bot:Bot,msg:Message,db:Db)->ResponseResult<()> {
    let Some(text)=msg.text() else{return respond(())};let Some(sender)=msg.from.as_ref() else{return respond(())};
    let chat=msg.chat.id.0;let user=match i64::try_from(sender.id.0){Ok(id)=>id,Err(_)=>return respond(())};
    let response=match db.lock(){Ok(mut conn)=>{let before:i64=conn.query_row("SELECT COALESCE(MAX(id),0) FROM events",[],|row|row.get(0)).unwrap_or(0);let result=execute(&mut conn,chat,user,&sender.full_name(),text);let created=if text.split_whitespace().next().unwrap_or("").split('@').next()==Some("/create") && conn.last_insert_rowid()!=before && result.is_ok(){Some((conn.last_insert_rowid(),language(&conn,chat).unwrap_or_default()=="ru"))}else{None};result.map(|text|(text,created))},Err(_)=>Err(anyhow!("database lock poisoned"))};
    match response {Ok((text,Some((id,ru)))) if !text.is_empty()=>{bot.send_message(msg.chat.id,text).reply_markup(keyboard(id,ru)).await?;},Ok((text,_)) if !text.is_empty()=>{bot.send_message(msg.chat.id,text).await?;},Err(e)=>{eprintln!("request error: {e:#}");bot.send_message(msg.chat.id,"Could not process this request. / Не удалось выполнить запрос.").await?;},_=>{}} respond(())
}
async fn callback_handler(bot:Bot,q:CallbackQuery,db:Db)->ResponseResult<()> {
    let answer=if let (Some(message),Some(data))=(q.regular_message(),q.data.as_deref()){
        let mut parts=data.split(':');let action=parts.next().unwrap_or("");let id=parts.next().and_then(|s|s.parse::<i64>().ok());
        if parts.next().is_none() && matches!(action,"join"|"leave"|"people") && id.is_some_and(|n|n>0){
            let chat=message.chat.id.0;let user=i64::try_from(q.from.id.0).unwrap_or(0);let command=format!("/{action} {}",id.unwrap_or_default());
            match db.lock(){Ok(mut conn)=>execute(&mut conn,chat,user,&q.from.full_name(),&command).unwrap_or_else(|e|{eprintln!("callback error: {e:#}");"Request failed / Ошибка запроса".into()}),Err(_)=>"Request failed / Ошибка запроса".into()}
        }else{"Invalid action / Неверное действие".into()}
    }else{"Message unavailable / Сообщение недоступно".into()};
    let short=answer.chars().take(190).collect::<String>();bot.answer_callback_query(q.id).text(short).show_alert(true).await?;respond(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let token=env::var("TELOXIDE_TOKEN").context("Set TELOXIDE_TOKEN to your Telegram bot token")?;
    let db=init_db(&env::var("GATHER_DB").unwrap_or_else(|_| "gather.sqlite".into()))?;
    let bot=Bot::new(token);tokio::spawn(reminders(bot.clone(),db.clone()));
    let handler=teloxide::dptree::entry().branch(Update::filter_message().endpoint(message_handler)).branch(Update::filter_callback_query().endpoint(callback_handler));
    Dispatcher::builder(bot,handler).dependencies(teloxide::dptree::deps![db]).enable_ctrlc_handler().build().dispatch().await;Ok(())
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn meetup_waitlist_and_chat_scope()->Result<()> {
  let db=init_db(":memory:")?;let mut db=db.lock().map_err(|_|anyhow!("lock"))?;
  let date=(Utc::now()+chrono::Duration::days(2)).to_rfc3339();let text=format!("/create Dinner | {date} | 1 | Cafe");execute(&mut db,1,11,"Alice",&text)?;
  let id=db.last_insert_rowid();assert!(execute(&mut db,1,11,"Alice",&format!("/join {id}"))?.contains("attending"));
  assert!(execute(&mut db,1,22,"Bob",&format!("/join {id}"))?.contains("Waitlist"));
  assert!(execute(&mut db,2,22,"Bob",&format!("/join {id}"))?.contains("not found"));
  assert!(execute(&mut db,1,11,"Alice",&format!("/leave {id}"))?.contains("Bob"));
  assert!(execute(&mut db,1,22,"Bob",&format!("/cancel {id}"))?.contains("organizer"));
  execute(&mut db,1,11,"Alice",&format!("/cancel {id}"))?;assert!(execute(&mut db,1,22,"Bob",&format!("/join {id}"))?.contains("not found")); Ok(())
 }
}
