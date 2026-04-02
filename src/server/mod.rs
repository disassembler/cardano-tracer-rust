//! Full `hermod-tracer` server implementation
//!
//! This module provides [`TracerServer`], which accepts trace connections from
//! Cardano nodes and routes them to file logs, Prometheus metrics, EKG polling,
//! and optional re-forwarding — feature-for-feature with the Haskell
//! `cardano-tracer` (excluding RTView and email alerts).
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                     TracerServer                         │
//! │                                                          │
//! │  ┌──────────┐   ┌──────────────┐   ┌─────────────────┐ │
//! │  │ Network  │   │  Prometheus  │   │ Log rotation    │ │
//! │  │ acceptor │   │  HTTP server │   │ background task │ │
//! │  └────┬─────┘   └──────────────┘   └─────────────────┘ │
//! │       │ per-connection                                   │
//! │  ┌────▼──────────────────────────────────────────────┐  │
//! │  │  handle_connection (one task per node)             │  │
//! │  │   ├─ trace loop  →  LogWriter + ReForwarder        │  │
//! │  │   ├─ EKG poller  →  NodeState::registry            │  │
//! │  │   └─ DataPoint idle (keeps channel alive)          │  │
//! │  └────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! Load a [`config::TracerConfig`] from a YAML file and pass it to
//! [`TracerServer::new`], then `.await` [`TracerServer::run`]:
//!
//! ```no_run
//! use hermod::server::{TracerServer, config::TracerConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = TracerConfig::from_file("hermod-tracer.yaml".as_ref())?;
//!     TracerServer::new(config).run().await
//! }
//! ```
//!
//! See `config/hermod-tracer.yaml` in the repository for a fully-annotated
//! example configuration.

pub mod acceptor;
pub mod config;
pub mod datapoint;
pub mod ekg;
pub mod logging;
pub mod node;
pub mod prometheus;
pub mod reforwarder;
pub mod rotation;
pub mod trace_handler;

use crate::forwarder::{ForwarderAddress, ForwarderConfig, TraceForwarder};
use crate::server::acceptor::run_network;
use crate::server::config::TracerConfig;
use crate::server::logging::LogWriter;
use crate::server::node::TracerState;
use crate::server::reforwarder::ReForwarder;
use crate::server::rotation::run_rotation_loop;
use std::sync::Arc;
use tracing::info;

/// The top-level tracer server
pub struct TracerServer {
    config: Arc<TracerConfig>,
    state: Arc<TracerState>,
}

impl TracerServer {
    /// Create a new server from the given config
    pub fn new(config: TracerConfig) -> Self {
        let config = Arc::new(config);
        let state = Arc::new(TracerState::new(config.clone()));
        TracerServer { config, state }
    }

    /// Run until cancelled
    pub async fn run(self) -> anyhow::Result<()> {
        info!("Starting hermod-tracer server");

        let config = self.config.clone();
        let state = self.state.clone();
        let writer = Arc::new(LogWriter::new());

        // --- Re-forwarder ---
        let reforwarder: Option<Arc<ReForwarder>> = if let Some(rf_cfg) = &config.has_forwarding {
            match &rf_cfg.network {
                crate::server::config::Network::AcceptAt(addr) => {
                    // hermod-tracer acts as the trace-forward FORWARDER, connecting
                    // out to the downstream acceptor's socket.
                    let fwd_address = match addr {
                        crate::server::config::Address::LocalPipe(p) => {
                            ForwarderAddress::Unix(p.clone())
                        }
                        crate::server::config::Address::RemoteSocket(host, port) => {
                            ForwarderAddress::Tcp(host.clone(), *port)
                        }
                    };
                    let fwd_config = ForwarderConfig {
                        address: fwd_address,
                        queue_size: rf_cfg.forwarder_opts.queue_size,
                        network_magic: config.network_magic as u64,
                        ..Default::default()
                    };
                    let forwarder = TraceForwarder::new(fwd_config);
                    let handle = forwarder.handle();
                    tokio::spawn(async move {
                        let _ = forwarder.run().await;
                    });
                    Some(Arc::new(ReForwarder::new(
                        handle,
                        rf_cfg.namespace_filters.clone(),
                    )))
                }
                crate::server::config::Network::ConnectTo(addrs) => {
                    // hermod-tracer listens; downstream acceptors connect to it.
                    // We broadcast forwarded traces to all connected downstreams.
                    let capacity = rf_cfg.forwarder_opts.queue_size.max(1);
                    let (tx, _) = tokio::sync::broadcast::channel(capacity);
                    let rf = Arc::new(ReForwarder::new_inbound(
                        tx.clone(),
                        rf_cfg.namespace_filters.clone(),
                    ));
                    let addrs = addrs.clone();
                    let network_magic = config.network_magic as u64;
                    tokio::spawn(async move {
                        crate::server::reforwarder::run_accepting_loop(&addrs, tx, network_magic)
                            .await;
                    });
                    Some(rf)
                }
            }
        } else {
            None
        };

        let mut tasks = tokio::task::JoinSet::new();

        // --- Network (accept/connect loop) ---
        {
            let state = state.clone();
            let writer = writer.clone();
            let rf = reforwarder.clone();
            let network = config.network.clone();
            tasks.spawn(async move {
                if let Err(e) = run_network(&network, state, writer, rf).await {
                    tracing::error!("Network loop error: {}", e);
                }
            });
        }

        // --- Prometheus HTTP server ---
        if let Some(ep) = config.has_prometheus.clone() {
            let state = state.clone();
            let labels = config.prometheus_labels.clone();
            let no_suffix = config.metrics_no_suffix.unwrap_or(false);
            tasks.spawn(async move {
                if let Err(e) =
                    prometheus::run_prometheus_server(ep, state, labels, no_suffix).await
                {
                    tracing::error!("Prometheus server error: {}", e);
                }
            });
        }

        // --- Log rotation ---
        if let Some(rot) = config.rotation.clone() {
            let writer = writer.clone();
            let state = state.clone();
            let logging = config.logging.clone();
            tasks.spawn(async move {
                run_rotation_loop(writer, state, rot, logging).await;
            });
        }

        // Wait for any task to finish (normally they run forever)
        tasks.join_next().await;
        Ok(())
    }
}
