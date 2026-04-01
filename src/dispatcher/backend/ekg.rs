//! EKG / Prometheus backend: pushes metrics to a Prometheus registry

use super::{Backend, DispatchMessage};
use crate::dispatcher::traits::Metric;
use anyhow::Result;
use async_trait::async_trait;
use prometheus::{CounterVec, GaugeVec, Opts, Registry};
use std::collections::HashMap;
use std::sync::Mutex;

/// Backend that pushes metrics to a Prometheus registry
pub struct EkgBackend {
    registry: Registry,
    /// Cache of registered gauges keyed by metric name
    gauges: Mutex<HashMap<String, GaugeVec>>,
    /// Cache of registered counters keyed by metric name
    counters: Mutex<HashMap<String, CounterVec>>,
}

impl EkgBackend {
    /// Create a new EKG backend with the given Prometheus registry
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            gauges: Mutex::new(HashMap::new()),
            counters: Mutex::new(HashMap::new()),
        }
    }

    fn record_metric(&self, metric: &Metric) -> Result<()> {
        match metric {
            Metric::IntM(name, value) => {
                let gauge = self.get_or_create_gauge(name)?;
                gauge.with_label_values(&[]).set(*value as f64);
            }
            Metric::DoubleM(name, value) => {
                let gauge = self.get_or_create_gauge(name)?;
                gauge.with_label_values(&[]).set(*value);
            }
            Metric::CounterM(name, init) => {
                let counter = self.get_or_create_counter(name)?;
                if let Some(v) = init {
                    counter.with_label_values(&[]).inc_by(*v as f64);
                } else {
                    counter.with_label_values(&[]).inc();
                }
            }
            Metric::PrometheusM(name, labels) => {
                let label_names: Vec<&str> = labels.iter().map(|(k, _)| k.as_str()).collect();
                let label_values: Vec<&str> = labels.iter().map(|(_, v)| v.as_str()).collect();
                let gauge = self.get_or_create_gauge_with_labels(name, &label_names)?;
                gauge.with_label_values(&label_values).set(1.0);
            }
        }
        Ok(())
    }

    fn get_or_create_gauge(&self, name: &str) -> Result<GaugeVec> {
        let mut gauges = self.gauges.lock().unwrap();
        if let Some(g) = gauges.get(name) {
            return Ok(g.clone());
        }
        let opts = Opts::new(sanitise_name(name), name.to_string());
        let gauge =
            GaugeVec::new(opts, &[]).map_err(|e| anyhow::anyhow!("creating gauge {name}: {e}"))?;
        self.registry
            .register(Box::new(gauge.clone()))
            .map_err(|e| anyhow::anyhow!("registering gauge {name}: {e}"))?;
        gauges.insert(name.to_string(), gauge.clone());
        Ok(gauge)
    }

    fn get_or_create_gauge_with_labels(&self, name: &str, labels: &[&str]) -> Result<GaugeVec> {
        let mut gauges = self.gauges.lock().unwrap();
        if let Some(g) = gauges.get(name) {
            return Ok(g.clone());
        }
        let opts = Opts::new(sanitise_name(name), name.to_string());
        let gauge = GaugeVec::new(opts, labels)
            .map_err(|e| anyhow::anyhow!("creating gauge {name}: {e}"))?;
        self.registry
            .register(Box::new(gauge.clone()))
            .map_err(|e| anyhow::anyhow!("registering gauge {name}: {e}"))?;
        gauges.insert(name.to_string(), gauge.clone());
        Ok(gauge)
    }

    fn get_or_create_counter(&self, name: &str) -> Result<CounterVec> {
        let mut counters = self.counters.lock().unwrap();
        if let Some(c) = counters.get(name) {
            return Ok(c.clone());
        }
        let opts = Opts::new(sanitise_name(name), name.to_string());
        let counter = CounterVec::new(opts, &[])
            .map_err(|e| anyhow::anyhow!("creating counter {name}: {e}"))?;
        self.registry
            .register(Box::new(counter.clone()))
            .map_err(|e| anyhow::anyhow!("registering counter {name}: {e}"))?;
        counters.insert(name.to_string(), counter.clone());
        Ok(counter)
    }
}

/// Convert an arbitrary metric name to a valid Prometheus metric name
fn sanitise_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[async_trait]
impl Backend for EkgBackend {
    async fn dispatch(&self, msg: &DispatchMessage) -> Result<()> {
        for metric in &msg.metrics {
            if let Err(e) = self.record_metric(metric) {
                tracing::warn!("EKG metric error for {}: {e}", metric.name());
            }
        }
        Ok(())
    }
}
