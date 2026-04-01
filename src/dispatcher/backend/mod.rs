//! Backend trait and dispatch message type

use crate::dispatcher::traits::Metric;
use crate::protocol::types::{DetailLevel, TraceObject};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

pub mod datapoint;
pub mod ekg;
pub mod forwarder;
pub mod stdout;

/// Message passed to each backend during dispatch
#[derive(Debug, Clone)]
pub struct DispatchMessage {
    /// The fully assembled `TraceObject` (for the Forwarder backend)
    pub trace_object: TraceObject,
    /// Pre-rendered human string (may be empty — backends fall back to machine JSON)
    pub human: String,
    /// Machine-readable JSON value
    pub machine: Value,
    /// Metrics emitted by this trace message
    pub metrics: Vec<Metric>,
    /// Detail level that was used to format `machine`
    pub detail: DetailLevel,
}

/// Trait implemented by every backend
#[async_trait]
pub trait Backend: Send + Sync {
    /// Dispatch a single trace message to this backend
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()>;
}
