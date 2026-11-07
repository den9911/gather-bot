use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    Ignored,
    Reply(String),
    Created { event_id:i64, ru:bool, text:String },
}
impl CommandOutcome {
    pub fn text(&self) -> &str {
        match self { Self::Ignored=>"", Self::Reply(text)|Self::Created{text,..}=>text }
    }
    pub fn created(&self) -> Option<(i64,bool)> {
        match self { Self::Created{event_id,ru,..}=>Some((*event_id,*ru)), _=>None }
    }
}
pub fn valid_input(text:&str) -> bool { text.encode_utf16().take(4097).count()<=4096 }
fn command_parts(text:&str) -> (&str,&str) {
    let mut parts=text.trim().splitn(2,char::is_whitespace);
    (parts.next().unwrap_or("").split('@').next().unwrap_or(""),parts.next().unwrap_or("").trim())
}
pub fn execute(db:&mut Connection,chat:i64,user:i64,name:&str,text:&str)->Result<CommandOutcome> {
    execute_message(db,chat,user,name,text,None)
}
pub fn execute_message(db:&mut Connection,chat:i64,user:i64,name:&str,text:&str,message_id:Option<i32>)->Result<CommandOutcome> {
    if !valid_input(text) { return Ok(CommandOutcome::Reply("Command is too long / Слишком длинная команда".into())); }
    if text.trim().is_empty() { return Ok(CommandOutcome::Ignored); }
    if user<=0 { return Ok(CommandOutcome::Reply("Sender unavailable / Отправитель недоступен".into())); }
    let (command,args)=command_parts(text);
    if command=="/create" {
        let ru=language(db,chat)?=="ru";
        let fingerprint=Sha256::digest(text.trim().as_bytes()).to_vec();
        return create(db,chat,user,args,message_id,&fingerprint,ru);
    }
    let bounded_name=name.chars().take(60).collect::<String>();
    let reply=execute_text(db,chat,user,&bounded_name,text)?;
    Ok(if reply.is_empty(){CommandOutcome::Ignored}else{CommandOutcome::Reply(reply)})
}
struct CreatedRow {
    id:i64, owner:i64, title:String, display_time:String, location:String,
    starts:i64, cancelled:bool, fingerprint:Vec<u8>,
}
fn creation_outcome(row:&CreatedRow,ru:bool,replayed:bool)->Result<CommandOutcome> {
    if replayed && (row.cancelled || row.starts<=Utc::now().timestamp()) {
        return Ok(CommandOutcome::Reply(format!("{} #{}; {}",words(ru,"Already created","Уже создана встреча"),row.id,words(ru,"this event is no longer open.","эта встреча больше не открыта."))));
    }
    let date=DateTime::parse_from_rfc3339(&row.display_time).context("Invalid saved creation time")?;
    let heading=if replayed {words(ru,"Already created","Уже создана встреча")} else {words(ru,"Created","Создана встреча")};
    Ok(CommandOutcome::Created {event_id:row.id,ru,text:format!("{} #{}: {}\n{}{}\n/join {} · /people {}",heading,row.id,row.title,date.format("%Y-%m-%d %H:%M %:z"),if row.location.is_empty(){String::new()}else{format!("\n📍 {}",row.location)},row.id,row.id)})
}
fn create(db:&mut Connection,chat:i64,user:i64,args:&str,message_id:Option<i32>,fingerprint:&[u8],ru:bool)->Result<CommandOutcome> {
    if message_id.is_some_and(|id|id<=0) { return Ok(CommandOutcome::Reply("Message identifier unavailable / Номер сообщения недоступен".into())); }
    let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if let Some(message_id)=message_id {
        let old=tx.query_row("SELECT id,owner,title,display_time,location,starts,cancelled,source_fingerprint FROM events WHERE chat_id=?1 AND source_message_id=?2",params![chat,message_id],|r|Ok(CreatedRow{id:r.get(0)?,owner:r.get(1)?,title:r.get(2)?,display_time:r.get(3)?,location:r.get(4)?,starts:r.get(5)?,cancelled:r.get(6)?,fingerprint:r.get(7)?})).optional()?;
        if let Some(old)=old {
            if old.owner!=user || old.fingerprint.as_slice()!=fingerprint { return Ok(CommandOutcome::Reply(words(ru,"This message already belongs to another create request. Check /list.","Это сообщение уже относится к другой заявке. Проверьте /list.").into())); }
            return creation_outcome(&old,ru,true);
        }
    }
    let fields:Vec<_>=args.split('|').map(str::trim).collect();
    if !(3..=4).contains(&fields.len()) || fields[0].is_empty() || fields[0].chars().count()>120 || fields.get(3).is_some_and(|s|s.chars().count()>200) {
        return Ok(CommandOutcome::Reply(words(ru,"Use /create title | date with offset | capacity","Формат: /create название | дата со смещением | количество мест").into()));
    }
    let date=match DateTime::parse_from_rfc3339(fields[1]) {Ok(date) if date.timestamp()>Utc::now().timestamp()=>date,_=>return Ok(CommandOutcome::Reply(words(ru,"Enter a future RFC 3339 date with offset.","Укажите будущую RFC 3339 дату со смещением.").into()))};
    let capacity:i64=match fields[2].parse(){Ok(n) if (1..=1000).contains(&n)=>n,_=>return Ok(CommandOutcome::Reply(words(ru,"Capacity must be between 1 and 1000.","Количество мест: от 1 до 1000.").into()))};
    let location=fields.get(3).copied().unwrap_or("");
    let display_time=date.to_rfc3339();
    let id=tx.query_row("INSERT INTO events(chat_id,owner,title,starts,capacity,location,display_time,source_message_id,source_fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) RETURNING id",params![chat,user,fields[0],date.timestamp(),capacity,location,display_time,message_id,fingerprint],|r|r.get(0))?;
    tx.commit()?;
    creation_outcome(&CreatedRow{id,owner:user,title:fields[0].into(),display_time,location:location.into(),starts:date.timestamp(),cancelled:false,fingerprint:fingerprint.to_vec()},ru,false)
}

pub fn language(db: &Connection, chat: i64) -> Result<String> {
    Ok(db.query_row("SELECT lang FROM chats WHERE chat_id=?1", [chat], |r| r.get(0)).optional()?.unwrap_or_else(|| "en".into()))
}
pub fn words(ru: bool, en: &'static str, russian: &'static str) -> &'static str { if ru { russian } else { en } }
fn execute_text(db: &mut Connection, chat: i64, user: i64, name: &str, text: &str) -> Result<String> {
    let ru = language(db, chat)? == "ru";
    let (command, args) = command_parts(text);
    match command {
        "/start" | "/help" => Ok(words(ru,
            "Gather Bot\n/create title | 2027-01-20T18:00:00+03:00 | capacity | place (optional)\n/list — upcoming events\n/join ID · /leave ID · /people ID · /cancel ID\n/calendar ID — calendar file\n/lang en or /lang ru\nThe date must include an explicit UTC offset. The bot must be running for reminders.",
            "Gather Bot\n/create название | 2027-01-20T18:00:00+03:00 | мест | место (необязательно)\n/list — будущие встречи\n/join ID · /leave ID · /people ID · /cancel ID\n/calendar ID — файл календаря\n/lang en или /lang ru\nУкажите дату с часовым смещением. Для напоминаний бот должен быть запущен.").into()),
        "/lang" => {
            if !["en", "ru"].contains(&args) { return Ok("Use /lang en or /lang ru".into()); }
            db.execute("INSERT INTO chats(chat_id,lang) VALUES(?1,?2) ON CONFLICT(chat_id) DO UPDATE SET lang=excluded.lang",params![chat,args])?;
            Ok(if args == "ru" { "Язык: русский" } else { "Language: English" }.into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_db;
    fn request() -> String { format!("/create Dinner | {} | 2 | Cafe",(Utc::now()+chrono::Duration::days(2)).to_rfc3339()) }
    #[test]
    fn unrelated_insert_cannot_turn_rejection_into_created_metadata() -> Result<()> {
        let shared=init_db(":memory:")?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        let before:i64=db.query_row("SELECT COALESCE(MAX(id),0) FROM events",[],|r|r.get(0))?;
        assert_ne!(db.last_insert_rowid(),before); // Reproduces the old transport predicate after app_meta insertion.
        let rejected=execute_message(&mut db,1,11,"Name","/create invalid",Some(7))?;
        assert!(matches!(&rejected,CommandOutcome::Reply(_)) && rejected.created().is_none());
        db.execute("INSERT INTO chats(chat_id,lang) VALUES(999,'en')",[])?;
        assert_eq!(db.last_insert_rowid(),999);
        assert!(execute_message(&mut db,1,11,"Name","/create invalid",Some(8))?.created().is_none());
        assert_eq!(execute(&mut db,1,11,"Name","")?,CommandOutcome::Ignored);
        assert_eq!(db.query_row("SELECT count(*) FROM events",[],|r|r.get::<_,i64>(0))?,0);
        Ok(())
    }
    #[test]
    fn creation_metadata_comes_from_the_insert_even_with_another_trigger_insert() -> Result<()> {
        let shared=init_db(":memory:")?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        db.execute_batch("CREATE TABLE audit(id INTEGER PRIMARY KEY,value TEXT); CREATE TRIGGER audit_event AFTER INSERT ON events BEGIN INSERT INTO audit(id,value) VALUES(1000+new.id,'fixture'); END;")?;
        let outcome=execute_message(&mut db,1,11,"Name",&request(),Some(10))?;
        let (id,ru)=outcome.created().context("created")?;
        assert_eq!(id,1);assert!(!ru);assert!(outcome.text().contains("#1"));
        assert_eq!(db.query_row("SELECT id FROM audit",[],|r|r.get::<_,i64>(0))?,1001);
        Ok(())
    }
    #[test]
    fn failed_creation_leaves_no_receipt_and_the_same_request_can_retry() -> Result<()> {
        let shared=init_db(":memory:")?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        db.execute_batch("CREATE TRIGGER reject_create BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'private fixture'); END;")?;
        let text=request();assert!(execute_message(&mut db,1,11,"Name",&text,Some(11)).is_err());
        assert_eq!(db.query_row("SELECT count(*) FROM events",[],|r|r.get::<_,i64>(0))?,0);
        db.execute("DROP TRIGGER reject_create",[])?;
        assert!(execute_message(&mut db,1,11,"Name",&text,Some(11))?.created().is_some());
        Ok(())
    }
    #[test]
    fn replay_survives_reopen_and_rejects_changed_owner_or_payload() -> Result<()> {
        let temp=tempfile::tempdir()?;let path=temp.path().join("gather.sqlite");let file=path.to_string_lossy();let text=request();
        let id={let shared=init_db(&file)?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;execute_message(&mut db,1,11,"Name",&text,Some(12))?.created().context("created")?.0};
        let shared=init_db(&file)?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        assert_eq!(execute_message(&mut db,1,11,"Name",&text,Some(12))?.created().context("replayed")?.0,id);
        assert!(execute_message(&mut db,1,12,"Other",&text,Some(12))?.created().is_none());
        assert!(execute_message(&mut db,1,11,"Name",&text.replace("Dinner","Changed"),Some(12))?.created().is_none());
        assert!(execute_message(&mut db,2,11,"Name",&text,Some(12))?.created().is_some());
        execute(&mut db,1,11,"Name",&format!("/cancel {id}"))?;
        assert!(execute_message(&mut db,1,11,"Name",&text,Some(12))?.created().is_none());
        assert_eq!(db.query_row("SELECT count(*) FROM events",[],|r|r.get::<_,i64>(0))?,2);
        assert_eq!(db.query_row("SELECT cancelled FROM events WHERE id=?1",[id],|r|r.get::<_,i64>(0))?,1);
        Ok(())
    }
    #[test]
    fn command_input_and_locale_are_explicit() -> Result<()> {
        let shared=init_db(":memory:")?;let mut db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        assert!(!valid_input(&"🦀".repeat(2049)));
        assert!(execute(&mut db,1,11,"Name",&"🦀".repeat(2049))?.created().is_none());
        execute(&mut db,1,11,"Name","/lang ru")?;
        let text=request().replace("/create ","/create@TestBot\t");
        let result=execute_message(&mut db,1,11,"Name",&text,Some(13))?;
        assert!(result.created().context("created")?.1);
        assert!(result.text().contains("Создана встреча"));
        assert!(execute_message(&mut db,1,11,"Name",&request(),Some(0))?.created().is_none());
        Ok(())
    }
    #[test]
    fn concurrent_connections_replay_one_creation() -> Result<()> {
        let temp=tempfile::tempdir()?;let path=temp.path().join("gather.sqlite");drop(init_db(&path.to_string_lossy())?);
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(2));let text=request();let mut handles=Vec::new();
        for _ in 0..2 {let path=path.clone();let barrier=barrier.clone();let text=text.clone();handles.push(std::thread::spawn(move || -> Result<i64> {let mut db=Connection::open(path)?;db.busy_timeout(std::time::Duration::from_secs(2))?;barrier.wait();Ok(execute_message(&mut db,1,11,"Name",&text,Some(14))?.created().context("created")?.0)}));}
        let ids=handles.into_iter().map(|h|h.join().map_err(|_|anyhow::anyhow!("thread"))?).collect::<Result<Vec<_>>>()?;
        assert_eq!(ids[0],ids[1]);let db=Connection::open(path)?;assert_eq!(db.query_row("SELECT count(*) FROM events",[],|r|r.get::<_,i64>(0))?,1);
        Ok(())
    }
    #[test]
    fn existing_event_migration_preserves_data_without_inventing_a_source_message() -> Result<()> {
        let temp=tempfile::tempdir()?;let path=temp.path().join("legacy.sqlite");
        {let db=Connection::open(&path)?;db.execute_batch("CREATE TABLE events(id INTEGER PRIMARY KEY,chat_id INTEGER NOT NULL,owner INTEGER NOT NULL,title TEXT NOT NULL,starts INTEGER NOT NULL,capacity INTEGER NOT NULL,cancelled INTEGER NOT NULL DEFAULT 0,reminded INTEGER NOT NULL DEFAULT 0); INSERT INTO events VALUES(1,1,11,'legacy',2000000000,2,0,0);")?;}
        let shared=init_db(&path.to_string_lossy())?;let db=shared.lock().map_err(|_|anyhow::anyhow!("lock"))?;
        assert_eq!(db.query_row("SELECT title FROM events WHERE id=1",[],|r|r.get::<_,String>(0))?,"legacy");
        assert!(db.query_row("SELECT source_message_id FROM events WHERE id=1",[],|r|r.get::<_,Option<i32>>(0))?.is_none());
        Ok(())
    }
}
