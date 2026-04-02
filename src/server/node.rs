//! Per-node state and shared tracer state
//!
//! Every Cardano node that connects to `hermod-tracer` gets a [`NodeState`]
//! instance, which holds:
//!
//! - A unique [`NodeId`] (the socket path or `ip:port` of the connection)
//! - A URL-safe [`NodeSlug`] derived from the node's display name for Prometheus routes
//! - A dedicated [`prometheus::Registry`] that accumulates EKG metrics for
//!   that node
//! - The connection timestamp
//!
//! All active nodes are tracked in the shared [`TracerState`], which is
//! `Arc`-cloned across every connection-handling task.

use crate::server::config::TracerConfig;
use indexmap::IndexMap;
use prometheus::{GaugeVec, Registry};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::RwLock;

/// Unique identifier for a connected node (socket path or ip:port)
pub type NodeId = String;

/// URL-safe slug derived from the node's display name, used as Prometheus route segment
pub type NodeSlug = String;

/// All state associated with one connected node
pub struct NodeState {
    /// The node's connection address (internal key — not shown to users)
    pub id: NodeId,
    /// Human-friendly display name from the node's `NodeInfo` DataPoint
    /// (`niName`). Falls back to the raw `NodeId` if the DataPoint request
    /// fails or returns an empty name.
    pub name: String,
    /// URL-safe slug derived from `name`, used in Prometheus routes and as
    /// the log subdirectory name
    pub slug: NodeSlug,
    /// This node's dedicated Prometheus registry
    pub registry: Arc<Registry>,
    /// When this node connected
    pub connected_at: Instant,
    /// Cache of Prometheus gauges derived from incoming trace object fields
    pub trace_gauge_cache: Mutex<HashMap<String, GaugeVec>>,
}

impl NodeState {
    /// Create new node state.
    ///
    /// `id` is the connection address (internal key).
    /// `name` is the display name (from `NodeInfo.niName`, fallback to `id`).
    pub fn new(id: NodeId, name: String) -> Self {
        let slug = slugify(&name);
        let registry = Arc::new(Registry::new());
        NodeState {
            id,
            name,
            slug,
            registry,
            connected_at: Instant::now(),
            trace_gauge_cache: Mutex::new(HashMap::new()),
        }
    }
}

/// State shared across all connections
pub struct TracerState {
    /// All currently-connected nodes, keyed by NodeId
    pub nodes: RwLock<IndexMap<NodeId, Arc<NodeState>>>,
    /// The loaded configuration
    pub config: Arc<TracerConfig>,
}

impl TracerState {
    /// Create a new empty tracer state
    pub fn new(config: Arc<TracerConfig>) -> Self {
        TracerState {
            nodes: RwLock::new(IndexMap::new()),
            config,
        }
    }

    /// Register a node; returns the new NodeState.
    ///
    /// `name` is the display name (from `NodeInfo.niName`).  Pass the same
    /// value as `id` when no name has been resolved yet.
    pub async fn register(&self, id: NodeId, name: String) -> Arc<NodeState> {
        let node = Arc::new(NodeState::new(id.clone(), name));
        self.nodes.write().await.insert(id, node.clone());
        node
    }

    /// Remove a node by ID
    pub async fn deregister(&self, id: &NodeId) {
        self.nodes.write().await.shift_remove(id);
    }

    /// Get a snapshot of connected nodes as (name, slug) pairs.
    ///
    /// `name` is the human-friendly display name (from `NodeInfo.niName`);
    /// `slug` is the URL-safe Prometheus route segment derived from it.
    pub async fn node_list(&self) -> Vec<(String, NodeSlug)> {
        self.nodes
            .read()
            .await
            .values()
            .map(|n| (n.name.clone(), n.slug.clone()))
            .collect()
    }

    /// Look up a node by slug
    pub async fn find_by_slug(&self, slug: &str) -> Option<Arc<NodeState>> {
        self.nodes
            .read()
            .await
            .values()
            .find(|n| n.slug == slug)
            .cloned()
    }

    /// Return all currently-connected nodes
    pub async fn all_nodes(&self) -> Vec<Arc<NodeState>> {
        self.nodes.read().await.values().cloned().collect()
    }
}

/// Convert an arbitrary string into a URL-safe slug:
/// lowercase, replace non-alphanumeric chars with `-`, collapse runs of `-`.
pub fn slugify(s: &str) -> String {
    let raw: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive dashes and trim leading/trailing dashes
    let mut result = String::with_capacity(raw.len());
    let mut last_was_dash = true; // skip leading dashes
    for c in raw.chars() {
        if c == '-' {
            if !last_was_dash {
                result.push('-');
                last_was_dash = true;
            }
        } else {
            result.push(c);
            last_was_dash = false;
        }
    }
    // Trim trailing dash
    if result.ends_with('-') {
        result.pop();
    }
    if result.is_empty() {
        result.push('x');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_unix_path() {
        assert_eq!(slugify("/tmp/forwarder.sock"), "tmp-forwarder-sock");
    }

    #[test]
    fn test_slugify_tcp() {
        assert_eq!(slugify("192.168.1.1:3000"), "192-168-1-1-3000");
    }

    #[test]
    fn test_slugify_already_clean() {
        assert_eq!(slugify("mynode"), "mynode");
    }

    #[test]
    fn test_slugify_empty_becomes_x() {
        assert_eq!(slugify("!!!"), "x");
    }
}
