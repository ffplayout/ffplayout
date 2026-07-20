use std::sync::Arc;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header,
    transport::smtp::authentication::Credentials,
};
use log::*;
use tokio::{
    sync::Mutex,
    time::{Duration, Instant, interval},
};

use crate::utils::{config::Mail, errors::ProcessError, round_to_nearest_ten};

const MAX_MAIL_LINES: usize = 1000;
const MAX_MAIL_RETRIES: u8 = 3;

#[derive(Clone, Debug)]
pub struct MailQueue {
    pub id: i32,
    pub config: Mail,
    pub lines: Vec<String>,
    pub raw_lines: Vec<String>,
    retry_attempts: u8,
    retry_after: Option<Instant>,
}

impl MailQueue {
    pub fn new(id: i32, config: Mail) -> Self {
        Self {
            id,
            config,
            lines: vec![],
            raw_lines: vec![],
            retry_attempts: 0,
            retry_after: None,
        }
    }

    pub fn level_eq(&self, level: Level) -> bool {
        level <= self.config.mail_level
    }

    pub fn update(&mut self, config: Mail) {
        self.config = config;
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    pub fn clear_raw(&mut self) {
        self.raw_lines.clear();
    }

    pub fn push(&mut self, line: String) {
        if self.lines.len() == MAX_MAIL_LINES {
            self.lines.remove(0);
        }
        self.lines.push(line);
    }

    pub fn push_raw(&mut self, line: String) {
        if self.raw_lines.len() == MAX_MAIL_LINES {
            self.raw_lines.remove(0);
        }
        self.raw_lines.push(line);
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    fn take_batch(&mut self) -> String {
        std::mem::take(&mut self.lines).join("\n")
    }

    fn retry_due(&self, now: Instant) -> bool {
        self.retry_after.is_some_and(|deadline| deadline <= now)
    }

    fn delivery_succeeded(&mut self) {
        self.retry_attempts = 0;
        self.retry_after = None;
    }

    fn delivery_failed(&mut self, batch: String, now: Instant) {
        self.retry_attempts += 1;
        if self.retry_attempts >= MAX_MAIL_RETRIES {
            self.delivery_succeeded();
            return;
        }

        let mut failed_lines = batch.lines().map(str::to_string).collect::<Vec<_>>();
        failed_lines.append(&mut self.lines);
        if failed_lines.len() > MAX_MAIL_LINES {
            failed_lines.drain(..failed_lines.len() - MAX_MAIL_LINES);
        }
        self.lines = failed_lines;
        self.retry_after = Some(now + Duration::from_secs(30 * (1 << (self.retry_attempts - 1))));
    }
}

/// send log messages to mail recipient
pub async fn send_mail(config: &Mail, msg: String, html: bool) -> Result<(), ProcessError> {
    let recipient = config
        .recipient
        .split_terminator([',', ';', ' '])
        .filter(|s| s.contains('@'))
        .map(str::trim)
        .collect::<Vec<&str>>();

    let mut message = Message::builder()
        .from(config.smtp_user.parse()?)
        .subject(&config.subject);

    message = match html {
        true => message.header(header::ContentType::TEXT_HTML),
        false => message.header(header::ContentType::TEXT_PLAIN),
    };

    for r in recipient {
        message = message.to(r.parse()?);
    }

    let mail = message.body(msg)?;
    let transporter = if config.smtp_starttls {
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_server)?
            .port(config.smtp_port)
    } else {
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_server)?.port(config.smtp_port)
    };

    let credentials = Credentials::new(config.smtp_user.clone(), config.smtp_password.clone());
    let mailer = transporter.credentials(credentials).build();

    // Send the mail
    mailer.send(mail).await?;

    Ok(())
}

/// Basic Mail Queue
///
/// Check every give seconds for messages and send them.
pub fn mail_queue(mail_queues: Arc<Mutex<Vec<Arc<Mutex<MailQueue>>>>>) {
    tokio::spawn(async move {
        let sec = 10;
        let mut interval = interval(Duration::from_secs(sec));
        let mut counter = 0;

        loop {
            interval.tick().await;
            let mut tasks = vec![];

            // Reset the counter after one day
            if counter >= 86400 {
                counter = 0;
            } else {
                counter += sec;
            }

            {
                let mut queues = mail_queues.lock().await;
                let now = Instant::now();

                // Process mail queues and send emails
                for queue in queues.iter_mut() {
                    let interval = round_to_nearest_ten(counter as i64);
                    let mut q_lock = queue.lock().await;

                    let expire = round_to_nearest_ten(q_lock.config.interval.max(30));
                    let retry_due = q_lock.retry_due(now);

                    if (retry_due || (q_lock.retry_after.is_none() && interval % expire == 0))
                        && !q_lock.is_empty()
                    {
                        if q_lock.config.recipient.contains('@') {
                            let config = q_lock.config.clone();
                            let id = q_lock.id;
                            let batch = q_lock.take_batch();
                            tasks.push((config, batch, id, queue.clone()));
                        } else {
                            q_lock.clear();
                            q_lock.delivery_succeeded();
                        }
                    }
                }
            }

            for (config, text, id, queue) in tasks {
                match send_mail(&config, text.clone(), false).await {
                    Ok(()) => queue.lock().await.delivery_succeeded(),
                    Err(error) => {
                        queue.lock().await.delivery_failed(text, Instant::now());
                        error!("Failed to send mail for channel {id}: {error}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_keeps_only_the_latest_lines() {
        let mut queue = MailQueue::new(1, Mail::default());
        for index in 0..=MAX_MAIL_LINES {
            queue.push(format!("line-{index}"));
            queue.push_raw(format!("raw-{index}"));
        }

        assert_eq!(queue.lines.len(), MAX_MAIL_LINES);
        assert_eq!(queue.raw_lines.len(), MAX_MAIL_LINES);
        assert_eq!(queue.lines.first().unwrap(), "line-1");
        assert_eq!(queue.raw_lines.first().unwrap(), "raw-1");
    }

    #[test]
    fn failed_delivery_retries_twice_then_discards_batch() {
        let now = Instant::now();
        let mut queue = MailQueue::new(1, Mail::default());

        queue.delivery_failed("first".to_string(), now);
        assert_eq!(queue.retry_attempts, 1);
        assert_eq!(queue.lines, ["first"]);
        assert!(!queue.retry_due(now));

        let batch = queue.take_batch();
        queue.delivery_failed(batch, now);
        assert_eq!(queue.retry_attempts, 2);
        assert_eq!(queue.lines, ["first"]);

        let batch = queue.take_batch();
        queue.delivery_failed(batch, now);
        assert_eq!(queue.retry_attempts, 0);
        assert!(queue.lines.is_empty());
        assert!(queue.retry_after.is_none());
    }
}
