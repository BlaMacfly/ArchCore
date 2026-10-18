//! Journalisation minimale avec horodatage (même format que le serveur Node).

use std::sync::atomic::{AtomicBool, Ordering};

static TRACE: AtomicBool = AtomicBool::new(false);

pub fn init() {
    TRACE.store(std::env::var("ARCHCORE_TRACE").as_deref() == Ok("1"), Ordering::Relaxed);
}

pub fn trace_enabled() -> bool {
    TRACE.load(Ordering::Relaxed)
}

pub fn stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { println!("{} INFO  {}", $crate::log::stamp(), format!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { eprintln!("{} WARN  {}", $crate::log::stamp(), format!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { eprintln!("{} ERROR {}", $crate::log::stamp(), format!($($arg)*)) };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::log::trace_enabled() { println!("{} DEBUG {}", $crate::log::stamp(), format!($($arg)*)) }
    };
}
