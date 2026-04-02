//! Datapoint backend — stores named data points for on-demand retrieval
//!
//! The `DatapointBackend` stores the most recent machine-JSON value for each
//! namespace key in a shared [`DataPointStore`].  The forwarder's DataPoint
//! mini-protocol handler reads from the same store when the acceptor queries a
//! named data point.
//!
//! # Usage
//!
//! ```no_run
//! use hermod::dispatcher::backend::datapoint::{DatapointBackend, DataPointStore};
//! use hermod::forwarder::TraceForwarder;
//! use std::sync::Arc;
//!
//! let store = DataPointStore::new();
//! let backend = DatapointBackend::with_store(store.clone());
//! let forwarder = TraceForwarder::new(Default::default())
//!     .with_datapoint_store(store);
//! // Wire `backend` into your Dispatcher and `forwarder` into your app.
//! ```

use super::{Backend, DispatchMessage};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Shared in-memory store for named data points.
///
/// Written by [`DatapointBackend::dispatch`] (keyed by the dot-joined
/// namespace of the trace object) and read by the forwarder's DataPoint
/// mini-protocol handler when the acceptor queries a named data point.
///
/// `DataPointStore` is cheap to clone — all clones share the same underlying
/// `HashMap`.
#[derive(Clone, Default)]
pub struct DataPointStore {
    inner: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl DataPointStore {
    /// Create a new empty store
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a raw JSON value under `name`
    pub fn put(&self, name: &str, value: Vec<u8>) {
        self.inner.write().unwrap().insert(name.to_string(), value);
    }

    /// Retrieve the value stored under `name`
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.inner.read().unwrap().get(name).cloned()
    }
}

/// Backend for the DataPoint protocol.
///
/// Each dispatched message is stored in the [`DataPointStore`] under its
/// dot-joined namespace key, overwriting any previous value for that key.
///
/// When no store is configured (the default created by
/// [`DispatcherBuilder::with_default_backends`]), messages are silently
/// discarded.  Use [`DatapointBackend::with_store`] with a shared
/// [`DataPointStore`] to enable full DataPoint support.
pub struct DatapointBackend {
    store: Option<DataPointStore>,
}

impl DatapointBackend {
    /// Create a no-op backend (all messages are silently discarded)
    pub fn new() -> Self {
        Self { store: None }
    }

    /// Create a backend that stores messages in `store`
    pub fn with_store(store: DataPointStore) -> Self {
        Self { store: Some(store) }
    }
}

impl Default for DatapointBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Backend for DatapointBackend {
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()> {
        if let Some(store) = &self.store {
            let key = msg.trace_object.to_namespace.join(".");
            let value = msg.trace_object.to_machine.as_bytes().to_vec();
            store.put(&key, value);
        }
        Ok(())
    }
}
