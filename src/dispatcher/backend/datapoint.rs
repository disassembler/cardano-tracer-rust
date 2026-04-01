//! Datapoint backend (stub)
//!
//! Not yet implemented. Logs a warning when dispatched to.

use super::{Backend, DispatchMessage};
use anyhow::Result;
use async_trait::async_trait;

/// Stub backend for the datapoint protocol
pub struct DatapointBackend;

#[async_trait]
impl Backend for DatapointBackend {
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()> {
        tracing::warn!(
            namespace = %msg.trace_object.to_namespace.join("."),
            "DatapointBackend is not yet implemented; message dropped"
        );
        Ok(())
    }
}
