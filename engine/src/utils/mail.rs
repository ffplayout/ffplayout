use std::sync::Arc;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header,
    transport::smtp::authentication::Credentials,
};
use log::*;
use tokio::{
    sync::Mutex,
    time::{Duration, interval},
};

use crate::utils::{config::Mail, errors::ProcessError, round_to_nearest_ten};

#[derive(Clone, Debug)]
pub struct MailQueue {
    pub id: i32,
    pub config: Mail,
    pub lines: Vec<String>,
    pub raw_lines: Vec<String>,
}

impl MailQueue {
    pub fn new(id: i32, config: Mail) -> Self {
        Self {
            id,
            config,
            lines: vec![],
            raw_lines: vec![],
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
        self.lines.push(line);
    }

    pub fn push_raw(&mut self, line: String) {
        self.raw_lines.push(line);
    }

    fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// send log messages to mail recipient
pub async fn send_mail(config: &Mail, msg: String) -> Result<(), ProcessError> {
    let recipient = config
        .recipient
        .split_terminator([',', ';', ' '])
        .filter(|s| s.contains('@'))
        .map(str::trim)
        .collect::<Vec<&str>>();

    let mut message = Message::builder()
        .from(config.smtp_user.parse()?)
        .subject(&config.subject)
        .header(header::ContentType::TEXT_PLAIN);

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

                // Process mail queues and send emails
                for queue in queues.iter_mut() {
                    let interval = round_to_nearest_ten(counter as i64);
                    let mut q_lock = queue.lock().await;

                    let expire = round_to_nearest_ten(q_lock.config.interval.max(30));

                    if interval % expire == 0 && !q_lock.is_empty() {
                        if q_lock.config.recipient.contains('@') {
                            tasks.push((q_lock.config.clone(), q_lock.text().clone(), q_lock.id));
                        }

                        // Clear the messages after sending the email
                        q_lock.clear();
                    }
                }
            }

            for (config, text, id) in tasks {
                if let Err(e) = send_mail(&config, text).await {
                    error!(target: "{file}", channel = id; "Failed to send mail: {e}");
                }
            }
        }
    });
}
