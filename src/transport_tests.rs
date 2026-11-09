use super::*;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::thread::JoinHandle;

type Captures = Arc<Mutex<Vec<(String, Value)>>>;
struct FakeApi {
    url: reqwest::Url,
    requests: Captures,
    mode: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl FakeApi {
    fn new(mode: u8) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let url = format!("http://{}/", listener.local_addr().expect("address"))
            .parse()
            .expect("URL");
        let requests: Captures = Arc::new(Mutex::new(Vec::new()));
        let mode = Arc::new(AtomicU8::new(mode));
        let stop = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let (captured, behavior, stopped, released) = (
            requests.clone(),
            mode.clone(),
            stop.clone(),
            release.clone(),
        );
        let worker = std::thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = serve(stream, &captured, &behavior, &stopped, &released);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            url,
            requests,
            mode,
            stop,
            release,
            worker: Some(worker),
        }
    }
    fn bot(&self) -> Bot {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(8))
            .build()
            .expect("test client");
        Bot::with_client("123456:fixture-token", client).set_api_url(self.url.clone())
    }
    fn captured(&self) -> Vec<(String, Value)> {
        self.requests.lock().expect("captures").clone()
    }
    async fn wait_request(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let count = self.requests.lock().expect("captures").len();
                if count > 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("request reached local fake API");
    }
}
impl Drop for FakeApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.release.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("fixture thread");
        }
    }
}
fn serve(
    mut stream: TcpStream,
    requests: &Captures,
    mode: &AtomicU8,
    stop: &AtomicBool,
    release: &AtomicBool,
) -> Result<()> {
    // Accepted sockets inherit nonblocking mode on some platforms.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let (method, payload) = {
        let mut input = BufReader::new(&mut stream);
        let mut line = String::new();
        input.read_line(&mut line)?;
        let method = line
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut length = 0;
        let mut header_bytes = line.len();
        loop {
            line.clear();
            input.read_line(&mut line)?;
            header_bytes += line.len();
            anyhow::ensure!(header_bytes < 16384, "fixture header bound");
            if line == "\r\n" {
                break;
            }
            anyhow::ensure!(!line.is_empty(), "incomplete HTTP request");
            if let Some((key, value)) = line.split_once(':') {
                if key.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>()?;
                }
            }
        }
        anyhow::ensure!(length <= 65536, "fixture body bound");
        let mut body = vec![0; length];
        input.read_exact(&mut body)?;
        (method, serde_json::from_slice::<Value>(&body)?)
    };
    {
        let mut values = requests.lock().expect("captures");
        anyhow::ensure!(values.len() < 128, "fixture capture bound");
        values.push((method.clone(), payload.clone()));
    }
    match mode.load(Ordering::SeqCst) {
        1 => return Ok(()), // Real connection closes after consuming the POST, without a response.
        2 => {
            while !release.load(Ordering::SeqCst) && !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        _ => {}
    }
    if stop.load(Ordering::SeqCst) {
        return Ok(());
    }
    let result = if method == "answercallbackquery" {
        json!(true)
    } else {
        json!({"message_id":999,"date":1700000000,"from":{"id":123456,"is_bot":true,"first_name":"Fixture Bot"},"chat":{"id":payload["chat_id"].as_i64().unwrap_or(42),"type":"private","first_name":"Fixture"},"text":payload["text"].as_str().unwrap_or("fixture")})
    };
    let body = serde_json::to_vec(&json!({"ok":true,"result":result}))?;
    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len())?;
    stream.write_all(&body)?;
    Ok(())
}
fn text() -> String {
    format!(
        "/create Dinner | {} | 2 | Cafe",
        (Utc::now() + chrono::Duration::days(2)).to_rfc3339()
    )
}
fn message(text: &str, id: i32) -> Message {
    serde_json::from_value(json!({"message_id":id,"date":1700000000,"from":{"id":12,"is_bot":false,"first_name":"Fixture"},"chat":{"id":42,"type":"private","first_name":"Fixture"},"text":text})).expect("message fixture")
}
fn callback(data: &str, regular: bool) -> CallbackQuery {
    let mut value = json!({"id":"fixture-query","from":{"id":99,"is_bot":false,"first_name":"Requester"},"chat_instance":"fixture-instance","data":data});
    if regular {
        value["message"] = serde_json::to_value(message("event", 5)).expect("callback message");
    } else {
        value["inline_message_id"] = json!("fixture-inline");
    }
    serde_json::from_value(value).expect("callback fixture")
}
fn count(db: &Db) -> i64 {
    db.lock()
        .expect("database")
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .expect("count")
}
fn keyboard_id(payload: &Value) -> Option<i64> {
    payload["reply_markup"]["inline_keyboard"][0][0]["callback_data"]
        .as_str()?
        .strip_prefix("join:")?
        .parse()
        .ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_response_then_reopen_replays_the_committed_event() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("gather.sqlite");
    let db = init_db(&path.to_string_lossy())?;
    let api = FakeApi::new(1);
    let msg = message(&text(), 20);
    assert!(message_handler(api.bot(), msg.clone(), db.clone())
        .await
        .is_err());
    assert!(!api.captured().is_empty());
    assert_eq!(count(&db), 1);
    assert_eq!(Arc::strong_count(&db), 1);
    drop(db);
    api.mode.store(0, Ordering::SeqCst);
    let db = init_db(&path.to_string_lossy())?;
    message_handler(api.bot(), msg, db.clone()).await?;
    assert_eq!(count(&db), 1);
    let captures = api.captured();
    let reply = &captures.last().context("reply")?.1;
    assert_eq!(keyboard_id(reply), Some(1));
    assert!(reply["text"]
        .as_str()
        .context("text")?
        .contains("Already created"));
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_releases_the_handler_without_rolling_back_commit() -> Result<()> {
    let db = init_db(":memory:")?;
    let api = FakeApi::new(2);
    let msg = message(&text(), 21);
    let pending = tokio::spawn(message_handler(api.bot(), msg.clone(), db.clone()));
    api.wait_request().await;
    assert_eq!(count(&db), 1);
    pending.abort();
    assert!(pending.await.expect_err("cancelled handler").is_cancelled());
    assert_eq!(Arc::strong_count(&db), 1);
    assert!(db.try_lock().is_ok());
    api.release.store(true, Ordering::SeqCst);
    api.mode.store(0, Ordering::SeqCst);
    message_handler(api.bot(), msg, db.clone()).await?;
    assert_eq!(count(&db), 1);
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_disconnects_release_resources_and_next_reply_works() -> Result<()> {
    let db = init_db(":memory:")?;
    let api = FakeApi::new(1);
    let msg = message(&text(), 22);
    for _ in 0..16 {
        assert!(message_handler(api.bot(), msg.clone(), db.clone())
            .await
            .is_err());
        assert_eq!(Arc::strong_count(&db), 1);
        assert!(db.try_lock().is_ok());
    }
    api.mode.store(0, Ordering::SeqCst);
    message_handler(api.bot(), msg, db.clone()).await?;
    assert_eq!(count(&db), 1);
    assert_eq!(
        keyboard_id(&api.captured().last().context("reply")?.1),
        Some(1)
    );
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_receipt_and_failed_insert_never_emit_a_false_keyboard() -> Result<()> {
    let db = init_db(":memory:")?;
    let api = FakeApi::new(0);
    let msg = message(&text(), 23);
    {
        db.lock().expect("db").execute_batch("CREATE TRIGGER fail_create BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'private SQL fixture'); END;")?;
    }
    message_handler(api.bot(), msg.clone(), db.clone()).await?;
    assert_eq!(count(&db), 0);
    let payload = api.captured().last().context("error reply")?.1.clone();
    assert!(keyboard_id(&payload).is_none());
    assert!(!payload.to_string().contains("private SQL fixture"));
    assert!(payload["text"].as_str().context("text")?.contains("/list"));
    {
        db.lock()
            .expect("db")
            .execute("DROP TRIGGER fail_create", [])?;
    }
    message_handler(api.bot(), msg.clone(), db.clone()).await?;
    assert_eq!(count(&db), 1);
    let fingerprint = {
        let conn = db.lock().expect("db");
        let bytes = conn.query_row("SELECT source_fingerprint FROM events", [], |r| {
            r.get::<_, Vec<u8>>(0)
        })?;
        conn.execute("UPDATE events SET source_fingerprint=NULL", [])?;
        bytes
    };
    message_handler(api.bot(), msg.clone(), db.clone()).await?;
    let payload = api.captured().last().context("error reply")?.1.clone();
    assert!(keyboard_id(&payload).is_none());
    assert!(!payload.to_string().contains("source_fingerprint"));
    assert_eq!(count(&db), 1);
    {
        db.lock().expect("db").execute(
            "UPDATE events SET source_fingerprint=?1",
            params![fingerprint],
        )?;
    }
    message_handler(api.bot(), msg, db.clone()).await?;
    assert_eq!(
        keyboard_id(&api.captured().last().context("reply")?.1),
        Some(1)
    );
    assert_eq!(count(&db), 1);
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn people_callback_sends_full_bounded_text_and_missing_message_is_clear() -> Result<()> {
    let db = init_db(":memory:")?;
    let api = FakeApi::new(0);
    {
        let mut conn = db.lock().expect("db");
        execute_message(&mut conn, 42, 12, "Fixture", &text(), Some(24))?;
        for id in 1..=50 {
            conn.execute(
                "INSERT INTO attendance VALUES(1,?1,?2,?3)",
                params![id, format!("{id}{}", "🦀".repeat(60)), id],
            )?;
        }
    }
    callback_handler(api.bot(), callback("people:1", true), db.clone()).await?;
    let values = api.captured();
    assert_eq!(values[0].0, "answercallbackquery");
    assert!(
        values[0].1["text"]
            .as_str()
            .context("ack")?
            .encode_utf16()
            .count()
            <= 190
    );
    let parts = values
        .iter()
        .filter(|(method, _)| method == "sendmessage")
        .map(|(_, value)| value["text"].as_str().expect("text"))
        .collect::<Vec<_>>();
    assert!((2..=4).contains(&parts.len()));
    assert!(parts.iter().all(|text| text.encode_utf16().count() <= 3500));
    let full = parts.concat();
    for id in 1..=50 {
        assert!(full.contains(&format!("{id}🦀")));
    }
    let before = values.len();
    callback_handler(api.bot(), callback("join:1", false), db.clone()).await?;
    let values = api.captured();
    assert_eq!(values.len(), before + 1);
    assert_eq!(values.last().context("ack")?.0, "answercallbackquery");
    assert!(values.last().context("ack")?.1["text"]
        .as_str()
        .context("text")?
        .contains("/list"));
    assert_eq!(count(&db), 1);
    Ok(())
}
#[test]
fn receipt_survives_real_process_exit() -> Result<()> {
    if let Ok(path) = env::var("GATHER_FIXTURE_DB") {
        let db = init_db(&path)?;
        let text = env::var("GATHER_FIXTURE_TEXT")?;
        let mut conn = db.lock().map_err(|_| anyhow!("lock"))?;
        execute_message(&mut conn, 42, 12, "Fixture", &text, Some(25))?;
        std::process::exit(91); // Intentionally skip Connection/transaction stack destructors after commit.
    }
    let temp = tempfile::Builder::new()
        .prefix("gather recovery ")
        .tempdir()?;
    let path = temp.path().join("gather.sqlite");
    let text = text();
    let output = std::process::Command::new(env::current_exe()?)
        .args([
            "--exact",
            "transport_tests::receipt_survives_real_process_exit",
            "--nocapture",
        ])
        .env("GATHER_FIXTURE_DB", &path)
        .env("GATHER_FIXTURE_TEXT", &text)
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(91),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let db = init_db(&path.to_string_lossy())?;
    let outcome = {
        let mut conn = db.lock().map_err(|_| anyhow!("lock"))?;
        execute_message(&mut conn, 42, 12, "Fixture", &text, Some(25))?
    };
    assert_eq!(outcome.created().context("receipt")?.0, 1);
    assert_eq!(count(&db), 1);
    assert_eq!(
        db.lock()
            .expect("db")
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))?,
        "ok"
    );
    Ok(())
}
