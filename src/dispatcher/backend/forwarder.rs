//! Forwarder backend: sends TraceObjects to hermod-tracer

use super::{Backend, DispatchMessage};
use crate::forwarder::ForwarderHandle;
use anyhow::Result;
use async_trait::async_trait;

/// Backend that forwards trace objects to hermod-tracer via the
/// trace-forward protocol.
pub struct ForwarderBackend {
    handle: ForwarderHandle,
}

impl ForwarderBackend {
    /// Create a new forwarder backend wrapping an existing handle
    pub fn new(handle: ForwarderHandle) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl Backend for ForwarderBackend {
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()> {
        // try_send is non-blocking; drops if the queue is full rather than blocking
        self.handle
            .try_send(msg.trace_object.clone())
            .map_err(|e| anyhow::anyhow!("forwarder queue full: {e}"))?;
        Ok(())
    }
}
