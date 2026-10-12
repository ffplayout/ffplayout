use log::*;
use std::io::Write;

use flexi_logger::writers::{FileLogWriter, LogWriter};
use flexi_logger::{Age, Cleanup, Criterion, DeferredNow, FileSpec, Logger, Naming};

pub fn file_logger() -> Box<dyn LogWriter> {
    Box::new(
        FileLogWriter::builder(
            FileSpec::default()
                .suppress_timestamp()
                .directory("./logs")
                .discriminant("1")
                .basename("ffplayout"),
        )
        .append()
        .format(file_formatter)
        .rotate(
            Criterion::Age(Age::Day),
            Naming::TimestampsCustomFormat {
                current_infix: Some(""),
                format: "%Y-%m-%d",
            },
            Cleanup::KeepLogFiles(4),
        )
        .print_message()
        .try_build()
        .unwrap(),
    )
}

fn file_formatter(
    w: &mut dyn Write,
    now: &mut DeferredNow,
    record: &Record,
) -> std::io::Result<()> {
    write!(
        w,
        "[{}] [{:>5}] {}",
        now.now().format("%Y-%m-%d %H:%M:%S%.6f"),
        record.level(),
        record.args()
    )
}

fn main() {
    let _logger = Logger::try_with_str("WARN")
        .expect("LogSpecification String has errors")
        .print_message()
        .log_to_stderr()
        .add_writer("Alert", file_logger())
        .start()
        .unwrap();

    error!(target : "{Alert,_Default}", "This is error message");
    warn!(target : "{Alert,_Default}", "This is a warning");
    info!(target : "{Alert,_Default}", "This is an info message");
    debug!(target : "{Alert,_Default}", "This is an debug message");
    trace!(target : "{Alert,_Default}", "This is an trace message");
}
