use std::{
    collections::HashMap,
    io,
    sync::{Arc, LazyLock, Mutex as StdMutex, PoisonError},
    time::{Duration, Instant},
};

use flexi_logger::{DeferredNow, writers::LogWriter};
use log::{Level, Record, error, kv::Value};
use tokio::{runtime::Handle, sync::Mutex};

use crate::utils::{
    config::Notification,
    logging::{is_fatal, strip_tags},
    mail::MailQueue,
};

static NOTIFICATION_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::new()
});
const NORMAL_WINDOW: Duration = Duration::from_secs(10 * 60);
const NORMAL_LIMIT: u8 = 5;
const FATAL_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const FINGERPRINT_COOLDOWN: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
struct NotificationRateLimit {
    channels: HashMap<i32, ChannelRateLimit>,
}

#[derive(Default)]
struct ChannelRateLimit {
    normal_window_started: Option<Instant>,
    normal_sent: u8,
    normal_suppressed: usize,
    normal_fingerprints: HashMap<String, Instant>,
    fatal_last_sent: Option<Instant>,
    fatal_suppressed: usize,
}

enum Permit {
    Send { suppressed: usize },
    Suppress,
}

impl NotificationRateLimit {
    fn permit(&mut self, channel: i32, fatal: bool, fingerprint: String, now: Instant) -> Permit {
        let state = self.channels.entry(channel).or_default();

        if fatal {
            if state
                .fatal_last_sent
                .is_some_and(|sent| now.duration_since(sent) < FATAL_COOLDOWN)
            {
                state.fatal_suppressed += 1;
                return Permit::Suppress;
            }

            state.fatal_last_sent = Some(now);
            return Permit::Send {
                suppressed: std::mem::take(&mut state.fatal_suppressed),
            };
        }

        state
            .normal_fingerprints
            .retain(|_, seen| now.duration_since(*seen) < FINGERPRINT_COOLDOWN);
        if state.normal_fingerprints.contains_key(&fingerprint) {
            state.normal_suppressed += 1;
            return Permit::Suppress;
        }
        state.normal_fingerprints.insert(fingerprint, now);

        if state
            .normal_window_started
            .is_none_or(|started| now.duration_since(started) >= NORMAL_WINDOW)
        {
            state.normal_window_started = Some(now);
            state.normal_sent = 0;
        }
        if state.normal_sent >= NORMAL_LIMIT {
            state.normal_suppressed += 1;
            return Permit::Suppress;
        }

        state.normal_sent += 1;
        Permit::Send {
            suppressed: std::mem::take(&mut state.normal_suppressed),
        }
    }
}

async fn send_notification(
    config: Notification,
    channel: i32,
    level: Level,
    fatal: bool,
    message: String,
) {
    let mut url = match reqwest::Url::parse(&config.server) {
        Ok(url) => url,
        Err(error) => {
            error!(target: "notification", "Invalid notification server for channel {channel}: {error}");
            return;
        }
    };
    let Ok(mut segments) = url.path_segments_mut() else {
        error!(target: "notification", "Notification server cannot be used as a base URL for channel {channel}");
        return;
    };
    segments.pop_if_empty().push(&config.topic);
    drop(segments);

    let priority = if fatal {
        "5"
    } else if level <= Level::Error {
        "4"
    } else if level <= Level::Warn {
        "3"
    } else {
        "2"
    };
    let title = format!(
        "ffplayout channel {channel} {}",
        if fatal { "FATAL" } else { level.as_str() }
    );

    let mut request = NOTIFICATION_CLIENT
        .post(url)
        .header("Title", title)
        .header("Priority", priority)
        .body(message);
    if !config.tags.is_empty() {
        request = request.header("Tags", config.tags);
    }
    if !config.token.is_empty() {
        request = request.bearer_auth(config.token);
    }

    if let Err(error) = request
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
    {
        error!(target: "notification", "Failed to send HTTP notification for channel {channel}: {error}");
    }
}

pub struct LogNotifier {
    queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
    rate_limit: Arc<StdMutex<NotificationRateLimit>>,
    runtime: Handle,
}

impl LogNotifier {
    pub fn new(queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) -> Self {
        Self {
            queues,
            rate_limit: Arc::new(StdMutex::new(NotificationRateLimit::default())),
            runtime: Handle::current(),
        }
    }

    async fn notify(&self, id: i32, level: Level, fatal: bool, message: String) {
        let config = {
            let queues = self.queues.lock().await;
            let mut config = None;
            for queue in queues.iter() {
                let queue = queue.lock().await;
                if queue.id != id {
                    continue;
                }
                if queue.notification.show
                    && !queue.notification.topic.is_empty()
                    && queue.notification.level.accepts(level, fatal)
                {
                    config = Some(queue.notification.clone());
                }
                break;
            }
            let Some(config) = config else {
                return;
            };
            config
        };

        let fingerprint = notification_fingerprint(&message);
        let permit = self
            .rate_limit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .permit(id, fatal, fingerprint, Instant::now());
        let Permit::Send { suppressed } = permit else {
            return;
        };
        let message = if suppressed == 0 {
            message
        } else {
            format!(
                "{suppressed} earlier notifications were suppressed by the rate limit.\n\n{message}"
            )
        };

        send_notification(config, id, level, fatal, message).await;
    }
}

impl LogWriter for LogNotifier {
    fn write(&self, _now: &mut DeferredNow, record: &Record<'_>) -> io::Result<()> {
        if record.target() == "notification" {
            return Ok(());
        }
        let id = i32::try_from(
            record
                .key_values()
                .get("channel".into())
                .and_then(|value| Value::to_i64(&value))
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let level = record.level();
        let fatal = is_fatal(record);
        let message = strip_tags(&record.args().to_string());

        let logger = Self {
            queues: self.queues.clone(),
            rate_limit: self.rate_limit.clone(),
            runtime: self.runtime.clone(),
        };
        self.runtime
            .spawn(async move { logger.notify(id, level, fatal, message).await });
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        Ok(())
    }
}

fn notification_fingerprint(message: &str) -> String {
    static MEMORY_ADDRESS: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"0x[0-9a-fA-F]+").unwrap());
    static LARGE_NUMBER: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\b-?\d{4,}\b").unwrap());
    static VALIDATION_POSITION: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r"(?m)(\[(?:Validator|Validation)\] (?:Engine probe error|Silence detection failed|Error)) on position \d+ - \d{2}:\d{2}:\d{2}(?:\.\d+)?:\s*",
        )
        .unwrap()
    });

    let message = MEMORY_ADDRESS.replace_all(message, "0x…");
    let message = LARGE_NUMBER.replace_all(&message, "#");
    VALIDATION_POSITION
        .replace_all(&message, "$1 ")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread, time::Duration};

    use flexi_logger::DeferredNow;
    use log::Record;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };

    use super::*;

    #[tokio::test]
    async fn sends_plain_text_notification_with_headers_and_bearer_token() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap_or_default();
                if request.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        send_notification(
            Notification {
                show: true,
                server: format!("http://{address}/base"),
                token: "secret".to_string(),
                topic: "alerts".to_string(),
                level: crate::utils::config::NotificationLevel::Error,
                tags: "warning,broadcast".to_string(),
            },
            7,
            Level::Error,
            false,
            "test message".to_string(),
        )
        .await;

        let request = receiver.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /base/alerts http/1.1\r\n"));
        assert!(request.contains("authorization: bearer secret\r\n"));
        assert!(request.contains("title: ffplayout channel 7 error\r\n"));
        assert!(request.contains("priority: 4\r\n"));
        assert!(request.contains("tags: warning,broadcast\r\n"));
        assert!(request.ends_with("test message"));
    }

    #[tokio::test]
    async fn sends_notifications_from_the_async_logger_thread() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });

        let queues = Arc::new(Mutex::new(vec![Arc::new(Mutex::new(MailQueue::new(
            0,
            crate::utils::config::Mail::default(),
            Notification {
                show: true,
                server: format!("http://{address}"),
                token: String::new(),
                topic: "alerts".to_string(),
                level: crate::utils::config::NotificationLevel::Error,
                tags: String::new(),
            },
        )))]));
        let notifier = Arc::new(LogNotifier::new(queues));

        thread::spawn(move || {
            let mut now = DeferredNow::new();
            let args = format_args!("background logger error");
            let record = Record::builder()
                .args(args)
                .level(Level::Error)
                .target("test")
                .build();
            notifier.write(&mut now, &record).unwrap();
        })
        .join()
        .unwrap();

        let request = timeout(Duration::from_secs(2), receiver)
            .await
            .expect("notification was not sent")
            .unwrap()
            .to_ascii_lowercase();
        assert!(request.starts_with("post /alerts http/1.1\r\n"));
        assert!(request.ends_with("background logger error"));
    }

    #[test]
    fn limits_normal_notifications_and_reports_suppressed_count() {
        let mut rate_limit = NotificationRateLimit::default();
        let now = Instant::now();

        for index in 0..NORMAL_LIMIT {
            assert!(matches!(
                rate_limit.permit(1, false, format!("message-{index}"), now),
                Permit::Send { suppressed: 0 }
            ));
        }
        assert!(matches!(
            rate_limit.permit(1, false, "another-message".to_string(), now),
            Permit::Suppress
        ));
        assert!(matches!(
            rate_limit.permit(1, false, "after-window".to_string(), now + NORMAL_WINDOW,),
            Permit::Send { suppressed: 1 }
        ));
    }

    #[test]
    fn limits_fatal_notifications_for_five_minutes() {
        let mut rate_limit = NotificationRateLimit::default();
        let now = Instant::now();

        assert!(matches!(
            rate_limit.permit(1, true, "first".to_string(), now),
            Permit::Send { suppressed: 0 }
        ));
        assert!(matches!(
            rate_limit.permit(1, true, "second".to_string(), now),
            Permit::Suppress
        ));
        assert!(matches!(
            rate_limit.permit(1, true, "third".to_string(), now + FATAL_COOLDOWN),
            Permit::Send { suppressed: 1 }
        ));
    }

    #[test]
    fn fingerprint_ignores_memory_addresses_and_large_numbers() {
        assert_eq!(
            notification_fingerprint("matroska @ 0x7f2cd59f42c0 failed PTS -3298446"),
            notification_fingerprint("matroska @ 0x123456789abc failed PTS -3298426")
        );
    }

    #[test]
    fn fingerprint_ignores_validation_position_and_time_but_keeps_the_source() {
        let first = notification_fingerprint(
            "[Validator] Engine probe error on position 001 - 00:03:24.480: /media/broken.mp4: Engine probe returned no media metadata",
        );
        let repeated = notification_fingerprint(
            "[Validator] Engine probe error on position 042 - 01:14:50.000: /media/broken.mp4: Engine probe returned no media metadata",
        );
        let other_source = notification_fingerprint(
            "[Validator] Engine probe error on position 042 - 01:14:50.000: /media/other.mp4: Engine probe returned no media metadata",
        );

        assert_eq!(first, repeated);
        assert_ne!(first, other_source);
    }
}
