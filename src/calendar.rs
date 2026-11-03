use anyhow::{Context,Result};
use chrono::{DateTime,Utc};
use rusqlite::{params,Connection,OptionalExtension};
fn escaped(value:&str)->String {value.replace('\\',"\\\\").replace("\r\n","\n").replace('\r',"\n").replace('\n',"\\n").replace(';',"\\;").replace(',',"\\,")}
fn line(value:&str)->String {
 let mut out=String::new();let mut bytes=0;
 for c in value.chars(){if bytes+c.len_utf8()>75{out.push_str("\r\n ");bytes=1;}out.push(c);bytes+=c.len_utf8();}
 out.push_str("\r\n");out
}
pub fn export(db:&Connection,chat:i64,id:i64)->Result<Option<Vec<u8>>> {
 let event:Option<(String,i64,String)>=db.query_row("SELECT title,starts,location FROM events WHERE id=?1 AND chat_id=?2 AND cancelled=0",params![id,chat],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
 let Some((title,starts,location))=event else{return Ok(None)};
 let instance:String=db.query_row("SELECT value FROM app_meta WHERE key='instance'",[],|r|r.get(0))?;
 let at=DateTime::from_timestamp(starts,0).context("Invalid event date")?;
 let mut text=String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Gather Bot//Local events//EN\r\nBEGIN:VEVENT\r\n");
 for value in [format!("UID:event-{id}-{instance}@gather-bot.local"),format!("DTSTAMP:{}",Utc::now().format("%Y%m%dT%H%M%SZ")),format!("DTSTART:{}",at.format("%Y%m%dT%H%M%SZ")),format!("SUMMARY:{}",escaped(&title)),format!("LOCATION:{}",escaped(&location))] {text.push_str(&line(&value));}
 text.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");Ok(Some(text.into_bytes()))
}
#[cfg(test)]mod tests{use super::*;
 #[test]fn utf8_folding_and_escaping(){let s=line(&format!("SUMMARY:{}",escaped(&"Привет, мир;\\\n".repeat(20))));assert!(s.split("\r\n").all(|l|l.len()<=75));assert!(s.replace("\r\n ","").contains("\\, мир\\;\\\\\\n"));}
}
