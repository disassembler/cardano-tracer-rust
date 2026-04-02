//! hermod::dispatcher — namespace-based trace routing layer
//!
//! The `Dispatcher` is the core component that sits between application code
//! (which calls `dispatch<M>()`) and the configured backends (stdout, forwarder,
//! EKG, datapoint).  It mirrors the Haskell `trace-dispatcher` library.
//!
//! # Usage
//!
//! ```rust,no_run
//! use hermod::dispatcher::{Dispatcher, DispatcherBuilder};
//! use hermod::dispatcher::config::TraceConfig;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let config = TraceConfig::default();
//! let dispatcher = DispatcherBuilder::new(config).build()?;
//! # Ok(())
//! # }
//! ```

pub mod backend;
pub mod config;
pub mod limiter;
pub mod traits;

use backend::{Backend, DispatchMessage};
use config::{BackendConfig, TraceConfig};
use limiter::TokenBucket;
use traits::{LogFormatting, MetaTrace, Privacy};

use crate::protocol::types::TraceObject;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The central dispatcher struct
///
/// Holds configuration, backend instances, and rate-limiter state.  It is
/// designed to be shared across threads via `Arc<Dispatcher>`.
pub struct Dispatcher {
    config: TraceConfig,
    backends: HashMap<BackendConfigKey, Arc<dyn Backend>>,
    limiters: Mutex<HashMap<Vec<String>, TokenBucket>>,
    hostname: String,
}

/// Stable key used to identify a backend instance in the map
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum BackendConfigKey {
    Forwarder,
    StdoutColoured,
    StdoutUncoloured,
    StdoutMachine,
    Ekg,
    Datapoint,
}

impl From<&BackendConfig> for BackendConfigKey {
    fn from(bc: &BackendConfig) -> Self {
        use config::FormatLogging;
        match bc {
            BackendConfig::Forwarder => BackendConfigKey::Forwarder,
            BackendConfig::Stdout(FormatLogging::HumanFormatColoured) => {
                BackendConfigKey::StdoutColoured
            }
            BackendConfig::Stdout(FormatLogging::HumanFormatUncoloured) => {
                BackendConfigKey::StdoutUncoloured
            }
            BackendConfig::Stdout(FormatLogging::MachineFormat) => BackendConfigKey::StdoutMachine,
            BackendConfig::EkgBackend => BackendConfigKey::Ekg,
            BackendConfig::DatapointBackend => BackendConfigKey::Datapoint,
        }
    }
}

impl Dispatcher {
    /// Dispatch a trace message.
    ///
    /// This is the main entry point for application code. The full pipeline is:
    ///
    /// 1. Compute namespace
    /// 2. Severity filter (drop if below threshold or Silence)
    /// 3. Rate limiter (drop if over frequency limit)
    /// 4. Format the message (human, machine, metrics)
    /// 5. Build `TraceObject`
    /// 6. Send to each configured backend (skipping Forwarder for Confidential)
    pub async fn dispatch<M>(&self, msg: &M)
    where
        M: MetaTrace + LogFormatting,
    {
        let ns = msg.namespace();
        let ns_complete = ns.complete();

        // --- Severity filter ---
        let severity_filter = self.config.severity_for(&ns_complete);
        let message_sev = match msg.severity() {
            Some(s) => s,
            None => {
                // No severity defined — treat as Debug
                crate::protocol::types::Severity::Debug
            }
        };
        if !severity_filter.passes(message_sev) {
            return;
        }

        // --- Rate limiter ---
        if let Some(max_freq) = self.config.limiter_for(&ns_complete) {
            let mut limiters = self.limiters.lock().unwrap();
            let bucket = limiters
                .entry(ns_complete.clone())
                .or_insert_with(|| TokenBucket::new(max_freq));
            if !bucket.try_acquire() {
                return;
            }
        }

        // --- Resolve config for this namespace ---
        let detail = self.config.detail_for(&ns_complete);
        let backends = self.config.backends_for(&ns_complete);
        let privacy = msg.privacy();

        // --- Format ---
        let human = msg.for_human();
        let machine_map = msg.for_machine(detail);
        let machine_value = serde_json::Value::Object(machine_map);
        let metrics = msg.as_metrics();

        // --- Build TraceObject ---
        let thread_id = format!("{:?}", std::thread::current().id());
        let trace_object = TraceObject {
            to_human: if human.is_empty() {
                None
            } else {
                Some(human.clone())
            },
            to_machine: machine_value.to_string(),
            to_namespace: ns_complete.clone(),
            to_severity: message_sev,
            to_details: detail,
            to_timestamp: Utc::now(),
            to_hostname: self.hostname.clone(),
            to_thread_id: thread_id,
        };

        let dispatch_msg = DispatchMessage {
            trace_object,
            human,
            machine: machine_value,
            metrics,
            detail,
        };

        // --- Send to backends ---
        for backend_cfg in &backends {
            // Confidential messages must not go to the Forwarder
            if privacy == Privacy::Confidential && *backend_cfg == BackendConfig::Forwarder {
                continue;
            }

            let key = BackendConfigKey::from(backend_cfg);
            if let Some(backend) = self.backends.get(&key) {
                if let Err(e) = backend.dispatch(&dispatch_msg).await {
                    tracing::warn!(
                        "Backend {:?} dispatch error for {}: {e}",
                        key,
                        ns_complete.join(".")
                    );
                }
            } else {
                tracing::debug!(
                    "No backend registered for {:?} (namespace {})",
                    key,
                    ns_complete.join(".")
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for constructing a `Dispatcher`
pub struct DispatcherBuilder {
    config: TraceConfig,
    backends: HashMap<BackendConfigKey, Arc<dyn Backend>>,
    hostname: Option<String>,
}

impl DispatcherBuilder {
    /// Create a builder with the given configuration
    pub fn new(config: TraceConfig) -> Self {
        Self {
            config,
            backends: HashMap::new(),
            hostname: None,
        }
    }

    /// Override the hostname (defaults to `hostname::get()`)
    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Register a backend for the `Stdout MachineFormat` backend config
    pub fn with_stdout_machine(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends
            .insert(BackendConfigKey::StdoutMachine, backend);
        self
    }

    /// Register a backend for `Stdout HumanFormatColoured`
    pub fn with_stdout_coloured(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends
            .insert(BackendConfigKey::StdoutColoured, backend);
        self
    }

    /// Register a backend for `Stdout HumanFormatUncoloured`
    pub fn with_stdout_uncoloured(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends
            .insert(BackendConfigKey::StdoutUncoloured, backend);
        self
    }

    /// Register the forwarder backend
    pub fn with_forwarder(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends.insert(BackendConfigKey::Forwarder, backend);
        self
    }

    /// Register the EKG/Prometheus backend
    pub fn with_ekg(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends.insert(BackendConfigKey::Ekg, backend);
        self
    }

    /// Register a datapoint backend
    pub fn with_datapoint(mut self, backend: Arc<dyn Backend>) -> Self {
        self.backends.insert(BackendConfigKey::Datapoint, backend);
        self
    }

    /// Automatically register all default backends based on the config
    ///
    /// This creates a `StdoutBackend` for each stdout format referenced in the
    /// config, a `DatapointBackend` stub for any datapoint entries, etc.
    /// The Forwarder backend must be registered separately via `with_forwarder`.
    pub fn with_default_backends(mut self) -> Self {
        use backend::{datapoint::DatapointBackend, stdout::StdoutBackend};
        use config::FormatLogging;

        // Collect all backend configs referenced anywhere in the options
        let mut seen: std::collections::HashSet<BackendConfigKey> =
            std::collections::HashSet::new();
        for opts in self.config.options.values() {
            for opt in opts {
                if let config::ConfigOption::Backends(bks) = opt {
                    for bk in bks {
                        seen.insert(BackendConfigKey::from(bk));
                    }
                }
            }
        }

        for key in seen {
            if self.backends.contains_key(&key) {
                continue;
            }
            match key {
                BackendConfigKey::StdoutMachine => {
                    self.backends.insert(
                        BackendConfigKey::StdoutMachine,
                        Arc::new(StdoutBackend::new(FormatLogging::MachineFormat)),
                    );
                }
                BackendConfigKey::StdoutColoured => {
                    self.backends.insert(
                        BackendConfigKey::StdoutColoured,
                        Arc::new(StdoutBackend::new(FormatLogging::HumanFormatColoured)),
                    );
                }
                BackendConfigKey::StdoutUncoloured => {
                    self.backends.insert(
                        BackendConfigKey::StdoutUncoloured,
                        Arc::new(StdoutBackend::new(FormatLogging::HumanFormatUncoloured)),
                    );
                }
                BackendConfigKey::Datapoint => {
                    self.backends.insert(
                        BackendConfigKey::Datapoint,
                        Arc::new(DatapointBackend::new()),
                    );
                }
                // Forwarder / EKG must be set up explicitly (they need external handles)
                _ => {}
            }
        }
        self
    }

    /// Build the `Dispatcher`
    pub fn build(self) -> anyhow::Result<Dispatcher> {
        let hostname = match self.hostname {
            Some(h) => h,
            None => hostname::get()
                .map(|h| h.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown".to_string()),
        };
        Ok(Dispatcher {
            config: self.config,
            backends: self.backends,
            limiters: Mutex::new(HashMap::new()),
            hostname,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatcher::{
        config::TraceConfig,
        traits::{LogFormatting, MetaTrace, Namespace, Privacy},
    };
    use crate::protocol::types::{DetailLevel, Severity};
    use serde_json::{Map, Value};
    use std::sync::atomic::{AtomicUsize, Ordering};

    // --- Test message type ---

    struct TestMsg {
        severity: Severity,
        text: String,
    }

    impl MetaTrace for TestMsg {
        fn namespace(&self) -> Namespace {
            Namespace::new(vec!["Test".to_string(), "Msg".to_string()])
        }
        fn severity(&self) -> Option<Severity> {
            Some(self.severity)
        }
        fn privacy(&self) -> Privacy {
            Privacy::Public
        }
    }

    impl LogFormatting for TestMsg {
        fn for_machine(&self, _detail: DetailLevel) -> Map<String, Value> {
            let mut m = Map::new();
            m.insert("text".to_string(), Value::String(self.text.clone()));
            m
        }
        fn for_human(&self) -> String {
            self.text.clone()
        }
    }

    // --- Counting backend ---

    struct CountingBackend(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl Backend for CountingBackend {
        async fn dispatch(&self, _msg: &DispatchMessage) -> anyhow::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn make_dispatcher(yaml: &str) -> (Dispatcher, Arc<AtomicUsize>) {
        let config = TraceConfig::from_yaml_str(yaml).unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let backend = Arc::new(CountingBackend(counter.clone()));
        let dispatcher = DispatcherBuilder::new(config)
            .with_hostname("test-host")
            .with_stdout_machine(backend)
            .build()
            .unwrap();
        (dispatcher, counter)
    }

    #[tokio::test]
    async fn test_severity_filter_passes() {
        let yaml = r#"
TraceOptions:
  "":
    severity: Info
    backends:
      - Stdout MachineFormat
"#;
        let (d, counter) = make_dispatcher(yaml);
        let msg = TestMsg {
            severity: Severity::Info,
            text: "hello".to_string(),
        };
        d.dispatch(&msg).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_severity_filter_blocks() {
        let yaml = r#"
TraceOptions:
  "":
    severity: Warning
    backends:
      - Stdout MachineFormat
"#;
        let (d, counter) = make_dispatcher(yaml);
        let msg = TestMsg {
            severity: Severity::Info,
            text: "filtered".to_string(),
        };
        d.dispatch(&msg).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_silence_blocks_all() {
        let yaml = r#"
TraceOptions:
  "":
    severity: Silence
    backends:
      - Stdout MachineFormat
"#;
        let (d, counter) = make_dispatcher(yaml);
        let msg = TestMsg {
            severity: Severity::Emergency,
            text: "blocked".to_string(),
        };
        d.dispatch(&msg).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let yaml = r#"
TraceOptions:
  "":
    severity: Debug
    backends:
      - Stdout MachineFormat
  Test.Msg:
    maxFrequency: 1.0
"#;
        let (d, counter) = make_dispatcher(yaml);
        let msg = TestMsg {
            severity: Severity::Info,
            text: "x".to_string(),
        };
        // First message should pass (full bucket)
        d.dispatch(&msg).await;
        // Second message immediately after should be rate-limited
        d.dispatch(&msg).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
