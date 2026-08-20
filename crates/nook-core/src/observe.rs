//! Island observability module.
//!
//! Prometheus HTTP is the only live source. Grafana, Alertmanager, and a later
//! fm-observe product are named on [`ObserveSourceKind`] so settings can point
//! at them later — they are not implemented here.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PINNED: usize = 6;
const MAX_ALERTS: usize = 8;
const MAX_SERIES: usize = 3;

/// Where the island should read metrics / outages from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObserveSourceKind {
    #[default]
    Prometheus,
    Grafana,
    Alertmanager,
    FmObserve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserveMetric {
    pub label: String,
    pub query: String,
}

impl ObserveMetric {
    pub fn new(label: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            query: query.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObserveConfig {
    #[serde(default)]
    pub source: ObserveSourceKind,
    #[serde(default)]
    pub prometheus_url: String,
    #[serde(default = "default_metrics")]
    pub metrics: Vec<ObserveMetric>,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            source: ObserveSourceKind::Prometheus,
            prometheus_url: String::new(),
            metrics: default_metrics(),
        }
    }
}

pub fn default_metrics() -> Vec<ObserveMetric> {
    vec![
        ObserveMetric::new("Targets", "count(up)"),
        ObserveMetric::new("Up", "sum(up)"),
        ObserveMetric::new("Memory", "process_resident_memory_bytes"),
    ]
}

#[derive(Debug, Clone, Default)]
pub struct ObserveSnapshot {
    pub connected: bool,
    pub error: Option<String>,
    pub metrics: Vec<MetricReading>,
    pub alerts: Vec<FiringAlert>,
}

impl ObserveSnapshot {
    pub fn firing_count(&self) -> usize {
        self.alerts.len()
    }

    pub fn has_outage(&self) -> bool {
        !self.alerts.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct MetricReading {
    pub label: String,
    pub query: String,
    pub values: Vec<SeriesValue>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SeriesValue {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Clone)]
pub struct FiringAlert {
    pub name: String,
    pub severity: String,
    pub summary: String,
}

pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return Err("Prometheus URL is empty".into());
    }
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return Err("Prometheus URL must start with http:// or https://".into());
    }
    if raw.contains([' ', '\n', '\r', '\t']) {
        return Err("Prometheus URL must not contain whitespace".into());
    }
    Ok(raw.to_string())
}

pub fn pin_metric(config: &mut ObserveConfig, label: &str, query: &str) -> Result<(), String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Metric query is empty".into());
    }
    if config
        .metrics
        .iter()
        .any(|m| m.query == query || m.label == label)
    {
        return Ok(());
    }
    if config.metrics.len() >= MAX_PINNED {
        return Err(format!("Pin at most {MAX_PINNED} metrics"));
    }
    let label = if label.trim().is_empty() {
        truncate_label(query)
    } else {
        label.trim().to_string()
    };
    config.metrics.push(ObserveMetric::new(label, query));
    Ok(())
}

pub fn unpin_metric(config: &mut ObserveConfig, query: &str) {
    config.metrics.retain(|m| m.query != query);
}

fn truncate_label(query: &str) -> String {
    let chars: String = query.chars().take(24).collect();
    if query.chars().count() > 24 {
        format!("{chars}…")
    } else {
        chars
    }
}

pub fn format_sample(value: f64) -> String {
    if !value.is_finite() {
        return "—".into();
    }
    let abs = value.abs();
    if abs >= 1_000_000_000.0 {
        format!("{:.2}G", value / 1_000_000_000.0)
    } else if abs >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if abs >= 1_000.0 {
        format!("{:.2}k", value / 1_000.0)
    } else if value.fract().abs() < 1e-9 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("openNook-gpui/observe")
        .build()
        .map_err(|e| e.to_string())
}

/// Poll the configured source. Grafana / Alertmanager / fm-observe return a
/// clear "not implemented" error instead of a fake empty dashboard.
pub async fn poll(config: &ObserveConfig) -> ObserveSnapshot {
    match config.source {
        ObserveSourceKind::Prometheus => poll_prometheus(config).await,
        other => ObserveSnapshot {
            error: Some(format!(
                "{other:?} is not wired yet — use Prometheus for this island widget"
            )),
            ..ObserveSnapshot::default()
        },
    }
}

async fn poll_prometheus(config: &ObserveConfig) -> ObserveSnapshot {
    let base = match normalize_base_url(&config.prometheus_url) {
        Ok(url) => url,
        Err(err) => {
            return ObserveSnapshot {
                error: Some(err),
                ..ObserveSnapshot::default()
            };
        }
    };
    let client = match client() {
        Ok(c) => c,
        Err(err) => {
            return ObserveSnapshot {
                error: Some(err),
                ..ObserveSnapshot::default()
            };
        }
    };

    let alerts = fetch_alerts(&client, &base).await;
    let (alerts, alert_error) = match alerts {
        Ok(list) => (list, None),
        Err(err) => (Vec::new(), Some(err)),
    };

    let mut metrics = Vec::new();
    for metric in config.metrics.iter().take(MAX_PINNED) {
        metrics.push(fetch_metric(&client, &base, metric).await);
    }

    let connected = alert_error.is_none() || metrics.iter().any(|m| m.error.is_none());
    ObserveSnapshot {
        connected,
        error: alert_error,
        metrics,
        alerts,
    }
}

async fn fetch_metric(client: &Client, base: &str, metric: &ObserveMetric) -> MetricReading {
    let url = format!("{base}/api/v1/query");
    match client
        .get(&url)
        .query(&[("query", metric.query.as_str())])
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => match parse_query(&body) {
                    Ok(values) => MetricReading {
                        label: metric.label.clone(),
                        query: metric.query.clone(),
                        values,
                        error: None,
                    },
                    Err(err) => MetricReading {
                        label: metric.label.clone(),
                        query: metric.query.clone(),
                        values: Vec::new(),
                        error: Some(if !status.is_success() {
                            format!("HTTP {status}: {err}")
                        } else {
                            err
                        }),
                    },
                },
                Err(err) => MetricReading {
                    label: metric.label.clone(),
                    query: metric.query.clone(),
                    values: Vec::new(),
                    error: Some(err.to_string()),
                },
            }
        }
        Err(err) => MetricReading {
            label: metric.label.clone(),
            query: metric.query.clone(),
            values: Vec::new(),
            error: Some(format_http_error(err)),
        },
    }
}

async fn fetch_alerts(client: &Client, base: &str) -> Result<Vec<FiringAlert>, String> {
    let url = format!("{base}/api/v1/alerts");
    let response = client.get(&url).send().await.map_err(format_http_error)?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Prometheus /api/v1/alerts returned HTTP {status}"));
    }
    parse_alerts(&body)
}

/// Metric names from Prometheus, used by Settings to pick pins.
pub async fn list_metric_names(config: &ObserveConfig) -> Result<Vec<String>, String> {
    if config.source != ObserveSourceKind::Prometheus {
        return Err("Metric browse is Prometheus-only for now".into());
    }
    let base = normalize_base_url(&config.prometheus_url)?;
    let client = client()?;
    let url = format!("{base}/api/v1/label/__name__/values");
    let response = client.get(&url).send().await.map_err(format_http_error)?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Prometheus label values returned HTTP {status}"));
    }
    parse_label_values(&body)
}

fn format_http_error(err: reqwest::Error) -> String {
    if err.is_connect() {
        format!("Can't reach Prometheus: {err}")
    } else if err.is_timeout() {
        "Prometheus timed out".into()
    } else {
        err.to_string()
    }
}

fn parse_query(body: &str) -> Result<Vec<SeriesValue>, String> {
    let parsed: PromEnvelope<PromQueryData> =
        serde_json::from_str(body).map_err(|e| format!("Unexpected query response: {e}"))?;
    if parsed.status != "success" {
        return Err(parsed
            .error
            .unwrap_or_else(|| "Prometheus query failed".into()));
    }
    let data = parsed
        .data
        .ok_or_else(|| "Query response missing data".to_string())?;
    match data.result_type.as_str() {
        "scalar" => {
            if let Some(value) = data.result.as_scalar() {
                Ok(vec![SeriesValue {
                    name: String::new(),
                    value,
                }])
            } else {
                Err("Scalar result was empty".into())
            }
        }
        "vector" => Ok(data
            .result
            .as_vector()
            .into_iter()
            .take(MAX_SERIES)
            .collect()),
        other => Err(format!("Unsupported result type '{other}'")),
    }
}

fn parse_alerts(body: &str) -> Result<Vec<FiringAlert>, String> {
    let parsed: PromEnvelope<PromAlertsData> =
        serde_json::from_str(body).map_err(|e| format!("Unexpected alerts response: {e}"))?;
    if parsed.status != "success" {
        return Err(parsed
            .error
            .unwrap_or_else(|| "Prometheus alerts failed".into()));
    }
    let data = parsed
        .data
        .ok_or_else(|| "Alerts response missing data".to_string())?;
    Ok(data
        .alerts
        .into_iter()
        .filter(|a| a.state.eq_ignore_ascii_case("firing"))
        .map(|a| FiringAlert {
            name: a
                .labels
                .get("alertname")
                .cloned()
                .unwrap_or_else(|| "alert".into()),
            severity: a
                .labels
                .get("severity")
                .cloned()
                .unwrap_or_else(|| "none".into()),
            summary: a
                .annotations
                .get("summary")
                .or_else(|| a.annotations.get("description"))
                .cloned()
                .unwrap_or_default(),
        })
        .take(MAX_ALERTS)
        .collect())
}

fn parse_label_values(body: &str) -> Result<Vec<String>, String> {
    let parsed: PromEnvelope<Vec<String>> =
        serde_json::from_str(body).map_err(|e| format!("Unexpected label response: {e}"))?;
    if parsed.status != "success" {
        return Err(parsed
            .error
            .unwrap_or_else(|| "Prometheus label values failed".into()));
    }
    let mut names = parsed.data.unwrap_or_default();
    names.retain(|n| !n.starts_with(':'));
    names.sort();
    names.truncate(80);
    Ok(names)
}

#[derive(Debug, Deserialize)]
struct PromEnvelope<T> {
    status: String,
    #[serde(default)]
    data: Option<T>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PromQueryData {
    #[serde(rename = "resultType", default)]
    result_type: String,
    #[serde(default)]
    result: PromResult,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PromResult {
    Vector(Vec<PromVectorSample>),
    Scalar(PromValue),
    Empty,
}

impl Default for PromResult {
    fn default() -> Self {
        Self::Empty
    }
}

impl PromResult {
    fn as_vector(self) -> Vec<SeriesValue> {
        match self {
            Self::Vector(rows) => rows
                .into_iter()
                .filter_map(|row| {
                    let value = row.value.value()?;
                    Some(SeriesValue {
                        name: series_name(&row.metric),
                        value,
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn as_scalar(self) -> Option<f64> {
        match self {
            Self::Scalar(v) => v.value(),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PromVectorSample {
    #[serde(default)]
    metric: serde_json::Map<String, serde_json::Value>,
    value: PromValue,
}

#[derive(Debug, Deserialize)]
struct PromValue(#[allow(dead_code)] serde_json::Value, serde_json::Value);

impl PromValue {
    fn value(&self) -> Option<f64> {
        match &self.1 {
            serde_json::Value::String(s) => s.parse().ok(),
            serde_json::Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct PromAlertsData {
    #[serde(default)]
    alerts: Vec<PromAlert>,
}

#[derive(Debug, Deserialize)]
struct PromAlert {
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
    #[serde(default)]
    annotations: std::collections::HashMap<String, String>,
    #[serde(default)]
    state: String,
}

fn series_name(metric: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut parts: Vec<String> = metric
        .iter()
        .filter(|(k, _)| *k != "__name__")
        .filter_map(|(k, v)| v.as_str().map(|s| format!("{k}={s}")))
        .collect();
    parts.sort();
    parts.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_non_http_urls() {
        assert!(normalize_base_url("").is_err());
        assert!(normalize_base_url("ftp://prom.local").is_err());
        assert!(normalize_base_url("http://prom.local /x").is_err());
        assert_eq!(
            normalize_base_url(" http://127.0.0.1:9090/ ").unwrap(),
            "http://127.0.0.1:9090"
        );
    }

    #[test]
    fn parses_vector_query() {
        let body = r#"{
            "status":"success",
            "data":{
                "resultType":"vector",
                "result":[{
                    "metric":{"__name__":"up","job":"prometheus","instance":"localhost:9090"},
                    "value":[1710000000,"1"]
                }]
            }
        }"#;
        let values = parse_query(body).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].value, 1.0);
        assert!(values[0].name.contains("job=prometheus"));
    }

    #[test]
    fn parses_scalar_query() {
        let body = r#"{
            "status":"success",
            "data":{"resultType":"scalar","result":[1710000000,"3.5"]}
        }"#;
        let values = parse_query(body).unwrap();
        assert_eq!(values[0].value, 3.5);
    }

    #[test]
    fn keeps_only_firing_alerts() {
        let body = r#"{
            "status":"success",
            "data":{"alerts":[
                {
                    "labels":{"alertname":"InstanceDown","severity":"critical"},
                    "annotations":{"summary":"target gone"},
                    "state":"firing"
                },
                {
                    "labels":{"alertname":"Soon"},
                    "annotations":{},
                    "state":"pending"
                }
            ]}
        }"#;
        let alerts = parse_alerts(body).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].name, "InstanceDown");
        assert_eq!(alerts[0].severity, "critical");
        assert_eq!(alerts[0].summary, "target gone");
    }

    #[test]
    fn pin_metric_dedupes_and_caps() {
        let mut config = ObserveConfig {
            metrics: Vec::new(),
            ..ObserveConfig::default()
        };
        config.metrics.clear();
        pin_metric(&mut config, "up", "up").unwrap();
        pin_metric(&mut config, "up", "up").unwrap();
        assert_eq!(config.metrics.len(), 1);
        for i in 0..MAX_PINNED {
            let _ = pin_metric(&mut config, &format!("m{i}"), &format!("q{i}"));
        }
        assert!(pin_metric(&mut config, "overflow", "overflow").is_err());
        assert!(config.metrics.len() <= MAX_PINNED);
    }

    #[test]
    fn formats_compact_numbers() {
        assert_eq!(format_sample(1.0), "1");
        assert_eq!(format_sample(1500.0), "1.50k");
        assert_eq!(format_sample(2_500_000.0), "2.50M");
    }

    #[test]
    fn source_enum_roundtrips() {
        let json = serde_json::to_string(&ObserveSourceKind::Prometheus).unwrap();
        assert_eq!(json, "\"prometheus\"");
        let _: ObserveSourceKind = serde_json::from_str("\"grafana\"").unwrap();
        let _: ObserveSourceKind = serde_json::from_str("\"alertmanager\"").unwrap();
        let _: ObserveSourceKind = serde_json::from_str("\"fm_observe\"").unwrap();
    }

    #[tokio::test]
    async fn polls_prometheus_http() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let mut stream = incoming.unwrap();
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let req = String::from_utf8_lossy(&buf);
                let body = if req.contains("/api/v1/alerts") {
                    r#"{"status":"success","data":{"alerts":[{"labels":{"alertname":"Down","severity":"critical"},"annotations":{"summary":"gone"},"state":"firing"}]}}"#
                } else {
                    r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{"__name__":"up"},"value":[1,"1"]}]}}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let config = ObserveConfig {
            prometheus_url: format!("http://{addr}"),
            metrics: vec![ObserveMetric::new("up", "up")],
            ..ObserveConfig::default()
        };
        let snap = poll(&config).await;
        assert!(
            snap.has_outage(),
            "expected firing alert, got {:?}",
            snap.error
        );
        assert_eq!(snap.alerts[0].name, "Down");
        assert_eq!(snap.metrics[0].values[0].value, 1.0);
    }
}
