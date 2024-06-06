use std::fmt;

use flexi_logger::{
    filter::{LogLineFilter, LogLineWriter},
    DeferredNow, FlexiLoggerError, Logger,
};
use log::info;
use log::kv::Key;

#[derive(Debug)]
enum Target {
    Terminal,
    File,
}

impl Target {
    fn as_str(&self) -> &'static str {
        match *self {
            Target::Terminal => "terminal",
            Target::File => "file",
        }
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Target::Terminal => write!(f, "terminal"),
            Target::File => write!(f, "file"),
        }
    }
}

pub struct Console;
impl LogLineFilter for Console {
    fn write(
        &self,
        now: &mut DeferredNow,
        record: &log::Record,
        log_line_writer: &dyn LogLineWriter,
    ) -> std::io::Result<()> {
        println!("{:?}", record.key_values().get(Key::from_str("target")));
        log_line_writer.write(now, record)?;
        Ok(())
    }
}

fn main() -> Result<(), FlexiLoggerError> {
    Logger::try_with_str("debug")?
        .filter(Box::new(Console))
        .start()?;

    info!(target: Target::Terminal.as_str(), "info logging");

    Ok(())
}
