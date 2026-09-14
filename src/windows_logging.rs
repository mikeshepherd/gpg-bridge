//! File logging for Windows services, which have no interactive stderr stream.

use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use log::{Level, LevelFilter, Log, Metadata, Record};

struct FileLogger {
    file: Mutex<File>,
    component: &'static str,
}

fn format_line(timestamp: u64, level: Level, component: &str, message: &str) -> String {
    format!("{timestamp} {level} [{component}] {message}")
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(
                file,
                "{}",
                format_line(
                    timestamp,
                    record.level(),
                    self.component,
                    &record.args().to_string(),
                )
            );
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Initializes an append-only logger suitable for a non-interactive Windows service.
pub fn init(path: &Path, component: &'static str) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    log::set_boxed_logger(Box::new(FileLogger {
        file: Mutex::new(file),
        component,
    }))
    .map_err(io::Error::other)?;
    log::set_max_level(LevelFilter::Info);
    Ok(())
}

#[cfg(test)]
mod tests {
    use log::Level;

    use super::format_line;

    #[test]
    fn formats_component_and_level_consistently() {
        assert_eq!(
            format_line(
                1_726_320_000,
                Level::Warn,
                "gpg-bridge-service",
                "agent unavailable"
            ),
            "1726320000 WARN [gpg-bridge-service] agent unavailable"
        );
    }
}
