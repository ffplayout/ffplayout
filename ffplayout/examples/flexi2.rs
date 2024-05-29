use log::*;
use std::io::Write;
// use std::io::{Error, ErrorKind};
// use std::sync::{Arc, Mutex};

use flexi_logger::writers::{FileLogWriter, LogWriter};
use flexi_logger::{Age, Cleanup, Criterion, DeferredNow, FileSpec, Logger, Naming};
use paris::formatter::colorize_string;

pub struct LogMailer;

impl LogWriter for LogMailer {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        println!("target: {:?}", record.target());
        println!("key/value: {:?}", record.key_values().get("channel".into()));
        println!(
            "[{}] [{:>5}] Mail logger: {:?}",
            now.now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.args()
        );
        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub struct LogConsole;

impl LogWriter for LogConsole {
    fn write(&self, now: &mut DeferredNow, record: &Record<'_>) -> std::io::Result<()> {
        console_formatter(&mut std::io::stderr(), now, record)?;

        println!();
        Ok(())
    }
    fn flush(&self) -> std::io::Result<()> {
        Ok(())
    }
}

pub fn file_logger(to_file: bool) -> Box<dyn LogWriter> {
    if to_file {
        Box::new(
            FileLogWriter::builder(
                FileSpec::default()
                    .suppress_timestamp()
                    // .directory("/var/log")
                    .basename("ffplayout"),
            )
            .append()
            .format(file_formatter)
            .rotate(
                Criterion::Age(Age::Day),
                Naming::Timestamps,
                Cleanup::KeepLogFiles(7),
            )
            .print_message()
            .try_build()
            .unwrap(),
        )
    } else {
        Box::new(LogConsole)
    }
}

// struct MyWriter<F> {
//     file: Arc<Mutex<F>>,
// }

// impl<F: std::io::Write + Send + Sync> LogWriter for MyWriter<F> {
//     fn write(
//         &self,
//         now: &mut flexi_logger::DeferredNow,
//         record: &flexi_logger::Record,
//     ) -> std::io::Result<()> {
//         let mut file = self
//             .file
//             .lock()
//             .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
//         flexi_logger::default_format(&mut *file, now, record)
//     }

//     fn flush(&self) -> std::io::Result<()> {
//         let mut file = self
//             .file
//             .lock()
//             .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
//         file.flush()
//     }
// }

// Define a macro for writing messages to the alert log and to the normal log
#[macro_use]
mod macros {
    #[macro_export]
    macro_rules! file_error {
        ($($arg:tt)*) => (
            error!(target: "{File}", $($arg)*);
        )
    }
}

pub fn console_formatter(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> std::io::Result<()> {
    let timestamp = colorize_string(format!(
        "<dimmed>[{}]</>",
        now.now().format("%Y-%m-%d %H:%M:%S%.6f")
    ));

    let level = match record.level() {
        Level::Debug => colorize_string("<bright magenta>[DEBUG]</>"),
        Level::Error => colorize_string("<bright red>[ERROR]</>"),
        Level::Info => colorize_string("<bright green>[ INFO]</>"),
        Level::Trace => colorize_string("<bright yellow>[TRACE]</>"),
        Level::Warn => colorize_string("<yellow>[ WARN]</>"),
    };

    write!(
        w,
        "{} {} {}",
        timestamp,
        level,
        colorize_string(record.args().to_string()),
    )
}

pub fn file_formatter(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> std::io::Result<()> {
    let timestamp = format!("<dimmed>[{}]</>", now.now().format("%Y-%m-%d %H:%M:%S%.6f"));

    let level = match record.level() {
        Level::Debug => "<magenta>[DEBUG]</>",
        Level::Error => "<red>[ERROR]</>",
        Level::Info => "<green>[ INFO]</>",
        Level::Trace => "<orange>[TRACE]</>",
        Level::Warn => "<yellow>[ WARN]</>",
    };

    write!(w, "{} {} {}", timestamp, level, record.args())
}

fn main() {
    let to_file = true;

    Logger::try_with_str("trace")
        .expect("LogSpecification String has errors")
        .format(console_formatter)
        .print_message()
        .log_to_stderr()
        .add_writer("File", file_logger(to_file))
        .add_writer("Mail", Box::new(LogMailer))
        .start()
        .unwrap();

    // Explicitly send logs to different loggers
    info!(target: "{Mail}", "This logs only to Mail");
    warn!(target: "{File,Mail}", channel = 1; "This logs to File and Mail");
    error!(target: "{File}", "This logs only to file");
    error!(target: "{_Default}", "This logs to console");

    file_error!("This is another file log");

    error!("This is a <bright red>normal error</> message");
    warn!("This is a warning");
    info!("This is an info message");
    debug!("This is an debug message");
    trace!("This is an trace message");
}
