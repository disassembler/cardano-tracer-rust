//! Stdout backend: writes formatted traces to standard output

use super::{Backend, DispatchMessage};
use crate::dispatcher::config::FormatLogging;
use anyhow::Result;
use async_trait::async_trait;

/// ANSI escape codes for colouring severity levels
mod colours {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RED: &str = "\x1b[31m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const CYAN: &str = "\x1b[36m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const WHITE: &str = "\x1b[37m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_WHITE: &str = "\x1b[97m";
}

/// Backend that writes trace messages to stdout
pub struct StdoutBackend {
    /// The formatting style to use
    pub format: FormatLogging,
}

impl StdoutBackend {
    /// Create a new stdout backend with the given format
    pub fn new(format: FormatLogging) -> Self {
        Self { format }
    }
}

#[async_trait]
impl Backend for StdoutBackend {
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()> {
        match self.format {
            FormatLogging::MachineFormat => {
                // Emit a single-line JSON object matching hermod-tracer's machine format
                let output = serde_json::to_string(&msg.machine)?;
                println!("{output}");
            }
            FormatLogging::HumanFormatColoured => {
                let text = build_human_line(msg);
                let coloured = apply_colour(&text, &msg.trace_object.to_severity);
                println!("{coloured}");
            }
            FormatLogging::HumanFormatUncoloured => {
                let text = build_human_line(msg);
                println!("{text}");
            }
        }
        Ok(())
    }
}

fn build_human_line(msg: &DispatchMessage) -> String {
    let obj = &msg.trace_object;
    let ts = obj.to_timestamp.format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let ns = obj.to_namespace.join(".");
    let sev = &obj.to_severity;

    let body = if msg.human.is_empty() {
        // Fall back to compact machine JSON wrapped in {"data": ...}
        format!(r#"{{"data":{}}}"#, msg.machine)
    } else {
        msg.human.clone()
    };

    format!("[{ts}] [{sev}] [{ns}] {body}")
}

fn apply_colour(text: &str, sev: &crate::protocol::types::Severity) -> String {
    use crate::protocol::types::Severity;
    use colours::*;
    let code = match sev {
        Severity::Debug => WHITE,
        Severity::Info => CYAN,
        Severity::Notice => BOLD,
        Severity::Warning => YELLOW,
        Severity::Error => RED,
        Severity::Critical => MAGENTA,
        Severity::Alert => BRIGHT_RED,
        Severity::Emergency => BRIGHT_WHITE,
    };
    format!("{code}{text}{RESET}")
}
