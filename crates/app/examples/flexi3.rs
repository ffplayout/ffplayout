use flexi_logger::writers::{FileLogWriter, LogWriter};
use flexi_logger::{Age, Cleanup, Criterion, DeferredNow, FileSpec, Naming, Record};
use log::{debug, error, info, kv::Value, trace, warn};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

struct MultiFileLogger {
    writers: Arc<Mutex<HashMap<String, Arc<Mutex<FileLogWriter>>>>>,
}

impl MultiFileLogger {
    pub fn new() -> Self {
        MultiFileLogger {
            writers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_writer(&self, channel: &str) -> io::Result<Arc<Mutex<FileLogWriter>>> {
        let mut writers = self.writers.lock().unwrap();
        if !writers.contains_key(channel) {
            let writer = FileLogWriter::builder(
                FileSpec::default()
                    .suppress_timestamp()
                    .basename("ffplayout"),
            )
            .append()
            .rotate(
                Criterion::Age(Age::Day),
                Naming::TimestampsCustomFormat {
                    current_infix: Some(""),
                    format: "%Y-%m-%d",
                },
                Cleanup::KeepLogFiles(7),
            )
            .print_message()
            .try_build()
            .map_err(|e| io::Error::other(e.to_string()))?;
            writers.insert(channel.to_string(), Arc::new(Mutex::new(writer)));
        }
        Ok(writers.get(channel).unwrap().clone())
    }
}

impl LogWriter for MultiFileLogger {
    fn write(&self, now: &mut DeferredNow, record: &Record) -> io::Result<()> {
        let channel = record
            .key_values()
            .get("channel".into())
            .unwrap_or(Value::null())
            .to_string();
        let writer = self.get_writer(&channel);

        writer?.lock().unwrap().write(now, record)
    }

    fn flush(&self) -> io::Result<()> {
        let writers = self.writers.lock().unwrap();
        for writer in writers.values() {
            writer.lock().unwrap().flush()?;
        }
        Ok(())
    }
}

fn main() {
    let logger = MultiFileLogger::new();

    flexi_logger::Logger::try_with_str("trace")
        .expect("LogSpecification String has errors")
        .print_message()
        .add_writer("file", Box::new(logger))
        .log_to_stderr()
        .start()
        .unwrap();

    trace!(target: "{file}", channel = 1; "This is a trace message for file1");
    trace!("This is a trace message for console");
    debug!(target: "{file}", channel = 2; "This is a debug message for file2");
    info!(target:"{file}", channel = 2; "This is an info message for file2");
    warn!(target: "{file}", channel = 1; "This is a warning for file1");
    error!(target: "{file}", channel = 2; "This is an error message for file2");
    info!("This is a info message for console");
}
