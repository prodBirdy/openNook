//! Island observability module.
//!
//! Live sources: warmUP `/admin/metrics` (default) and Prometheus HTTP.
//! Grafana, Alertmanager, and a later fm-observe product are named on
//! [`ObserveSourceKind`] so settings can point at them later.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PINNED: usize = 6;
const MAX_ALERTS: usize = 8;
const MAX_SERIES: usize = 3;
const WARMUP_METRICS_PATH: &str = "/admin/metrics";
const CHART_POINTS: usize = 60;
const STORE_SLACK_MS: u64 = 120_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Don't turn a counter into a rate across a long outage; the next poll starts a new segment.
const RATE_GAP_MS: u64 = 3 * 60 * 1000;
/// Until we have a full window of samples, zoom the x-axis to the collected span
/// so a few minutes of data is not a sliver on the right of a 30 min / 24 h plot.
const MIN_PLOT_MS: u64 = 2 * 60 * 1000;
const SAMPLES_TABLE: &str = "CREATE TABLE IF NOT EXISTS observe_samples (
    query TEXT NOT NULL,
    at INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (query, at)
)";

/// Production warmUP API used as the default metrics host while this widget
/// is wired up.
pub const DEFAULT_OBSERVE_URL: &str = "https://api.warmup-gamelauncher.com";

/// Where the island should read metrics / outages from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObserveSourceKind {
    #[default]
    Warmup,
    Prometheus,
    Grafana,
    Alertmanager,
    FmObserve,
}

/// Small chart drawn next to a pinned metric on the island.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObserveChartKind {
    Off,
    #[default]
    Sparkline,
    Bars,
}

impl ObserveChartKind {
    pub fn next(self) -> Self {
        match self {
            Self::Sparkline => Self::Bars,
            Self::Bars => Self::Off,
            Self::Off => Self::Sparkline,
        }
    }

    pub fn caption(self) -> &'static str {
        match self {
            Self::Sparkline => "Line",
            Self::Bars => "Bars",
            Self::Off => "No chart",
        }
    }
}

/// How far back the island chart should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ObserveWindow {
    #[default]
    ThirtyMinutes,
    FiveHours,
    OneDay,
}

impl ObserveWindow {
    pub fn next(self) -> Self {
        match self {
            Self::ThirtyMinutes => Self::FiveHours,
            Self::FiveHours => Self::OneDay,
            Self::OneDay => Self::ThirtyMinutes,
        }
    }

    pub fn caption(self) -> &'static str {
        match self {
            Self::ThirtyMinutes => "30 min",
            Self::FiveHours => "5 h",
            Self::OneDay => "24 h",
        }
    }

    pub fn duration_ms(self) -> u64 {
        match self {
            Self::ThirtyMinutes => 30 * 60 * 1000,
            Self::FiveHours => 5 * 60 * 60 * 1000,
            Self::OneDay => DAY_MS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserveMetric {
    pub label: String,
    pub query: String,
    #[serde(default)]
    pub chart: ObserveChartKind,
}

impl ObserveMetric {
    pub fn new(label: impl Into<String>, query: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            query: query.into(),
            chart: ObserveChartKind::Sparkline,
        }
    }

    pub fn with_chart(mut self, chart: ObserveChartKind) -> Self {
        self.chart = chart;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ObserveConfig {
    #[serde(default)]
    pub source: ObserveSourceKind,
    #[serde(default = "default_observe_url")]
    pub prometheus_url: String,
    /// Bearer token for warmUP `/admin/metrics`. Also read from
    /// `WARMUP_METRICS_TOKEN` / `ADMIN_METRICS_TOKEN` when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub metrics_token: String,
    #[serde(default)]
    pub window: ObserveWindow,
    #[serde(default = "default_metrics")]
    pub metrics: Vec<ObserveMetric>,
}

impl Default for ObserveConfig {
    fn default() -> Self {
        Self {
            source: ObserveSourceKind::Warmup,
            prometheus_url: default_observe_url(),
            metrics_token: String::new(),
            window: ObserveWindow::ThirtyMinutes,
            metrics: default_metrics(),
        }
    }
}

pub fn default_observe_url() -> String {
    DEFAULT_OBSERVE_URL.to_string()
}

pub fn default_metrics() -> Vec<ObserveMetric> {
    default_warmup_metrics()
}

fn default_warmup_metrics() -> Vec<ObserveMetric> {
    vec![
        ObserveMetric::new("Requests", "total_requests"),
        ObserveMetric::new("5xx", "5xx"),
        ObserveMetric::new("Slow", "slow"),
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
    pub chart: ObserveChartKind,
    pub values: Vec<SeriesValue>,
    pub error: Option<String>,
    pub history: Vec<ChartPoint>,
    /// For counters: increase over the selected window. Gauges leave this empty.
    pub window_total: Option<f64>,
}

/// One chart sample. `t` is 0 at the start of the selected window and 1 at now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChartPoint {
    pub t: f32,
    pub value: f64,
}

impl MetricReading {
    fn from_metric(
        metric: &ObserveMetric,
        values: Vec<SeriesValue>,
        error: Option<String>,
    ) -> Self {
        Self {
            label: metric.label.clone(),
            query: metric.query.clone(),
            chart: metric.chart,
            values,
            error,
            history: Vec::new(),
            window_total: None,
        }
    }

    pub fn headline(&self) -> String {
        if let Some(err) = &self.error {
            return err.clone();
        }
        if let Some(total) = self.window_total {
            return format_sample(total);
        }
        if let Some(first) = self.values.first() {
            let extra = if self.values.len() > 1 {
                format!(" (+{})", self.values.len() - 1)
            } else {
                String::new()
            };
            return format!("{}{extra}", format_sample(first.value));
        }
        "—".into()
    }
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
        return Err("Metrics URL is empty".into());
    }
    if !(raw.starts_with("http://") || raw.starts_with("https://")) {
        return Err("Metrics URL must start with http:// or https://".into());
    }
    if raw.contains([' ', '\n', '\r', '\t']) {
        return Err("Metrics URL must not contain whitespace".into());
    }
    Ok(raw.to_string())
}

/// Fill an empty saved URL with the warmUP default so first-run settings poll
/// the backend without a Settings visit.
pub fn fill_default_url(config: &mut ObserveConfig) {
    if config.prometheus_url.trim().is_empty() {
        config.prometheus_url = default_observe_url();
    }
}

fn is_warmup_host(raw: &str) -> bool {
    raw.to_ascii_lowercase().contains("warmup-gamelauncher.com")
}

fn uses_warmup(config: &ObserveConfig) -> bool {
    config.source == ObserveSourceKind::Warmup || is_warmup_host(&config.prometheus_url)
}

fn metrics_token(config: &ObserveConfig) -> String {
    let from_config = config.metrics_token.trim();
    if !from_config.is_empty() {
        return from_config.to_string();
    }
    std::env::var("WARMUP_METRICS_TOKEN")
        .or_else(|_| std::env::var("ADMIN_METRICS_TOKEN"))
        .unwrap_or_default()
        .trim()
        .to_string()
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

pub fn cycle_metric_chart(config: &mut ObserveConfig, query: &str) {
    if let Some(metric) = config.metrics.iter_mut().find(|m| m.query == query) {
        metric.chart = metric.chart.next();
    }
}

pub fn set_window(config: &mut ObserveConfig, window: ObserveWindow) {
    config.window = window;
}

pub type MetricHistory = HashMap<String, Vec<(u64, f64)>>;

/// Append this poll's samples onto `store` and copy a chart series onto each
/// reading. Counters are stored raw and charted as per-minute rates so a 30
/// min / 5 h window stays comparable.
pub fn record_history(
    store: &mut MetricHistory,
    snap: &mut ObserveSnapshot,
    window: ObserveWindow,
) {
    record_history_at(store, snap, window, now_ms(), true);
}

pub fn record_history_at(
    store: &mut MetricHistory,
    snap: &mut ObserveSnapshot,
    window: ObserveWindow,
    now: u64,
    persist: bool,
) {
    let retain_after = now.saturating_sub(DAY_MS + STORE_SLACK_MS);
    let mut fresh = Vec::new();
    for reading in &mut snap.metrics {
        if let Some(sample) = reading.values.first() {
            if reading.error.is_none() && sample.value.is_finite() {
                let hist = store.entry(reading.query.clone()).or_default();
                hist.push((now, sample.value));
                hist.retain(|(t, _)| *t >= retain_after);
                fresh.push((reading.query.clone(), now, sample.value));
            }
        }
        let hist = store.get(&reading.query).map(Vec::as_slice).unwrap_or(&[]);
        reading.history = chart_values(&reading.query, hist, window, now);
        reading.window_total = window_count(&reading.query, hist, window, now);
    }
    if persist {
        persist_samples(&fresh, retain_after);
    }
}

/// Last 24 hours of collected samples, for charts after a relaunch.
pub fn load_history() -> MetricHistory {
    let Ok(conn) = crate::database::get_connection() else {
        return MetricHistory::new();
    };
    if let Err(err) = conn.execute(SAMPLES_TABLE, []) {
        log::warn!("observe samples table: {err}");
        return MetricHistory::new();
    }
    load_history_from(&conn, now_ms())
}

fn load_history_from(conn: &rusqlite::Connection, now: u64) -> MetricHistory {
    let start = now.saturating_sub(DAY_MS + STORE_SLACK_MS);
    let mut store = MetricHistory::new();
    let Ok(mut stmt) = conn
        .prepare("SELECT query, at, value FROM observe_samples WHERE at >= ?1 ORDER BY query, at")
    else {
        return store;
    };
    let Ok(rows) = stmt.query_map([start as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)? as u64,
            row.get::<_, f64>(2)?,
        ))
    }) else {
        return store;
    };
    for row in rows.flatten() {
        store.entry(row.0).or_default().push((row.1, row.2));
    }
    store
}

fn persist_samples(fresh: &[(String, u64, f64)], retain_after: u64) {
    let Ok(conn) = crate::database::get_connection() else {
        return;
    };
    if let Err(err) = conn.execute(SAMPLES_TABLE, []) {
        log::warn!("observe samples table: {err}");
        return;
    }
    if let Err(err) = write_samples(&conn, fresh, retain_after) {
        log::warn!("observe samples write: {err}");
    }
}

fn write_samples(
    conn: &rusqlite::Connection,
    fresh: &[(String, u64, f64)],
    retain_after: u64,
) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT OR REPLACE INTO observe_samples (query, at, value) VALUES (?1, ?2, ?3)",
        )?;
        for (query, at, value) in fresh {
            insert.execute(rusqlite::params![query, *at as i64, value])?;
        }
    }
    tx.execute(
        "DELETE FROM observe_samples WHERE at < ?1",
        [retain_after as i64],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn chart_values(
    query: &str,
    history: &[(u64, f64)],
    window: ObserveWindow,
    now: u64,
) -> Vec<ChartPoint> {
    let (start, span) = plot_range(history, window, now);
    let t_of = |ms: u64| ((ms.saturating_sub(start) as f32) / span as f32).clamp(0.0, 1.0);

    let points = if is_counter_query(query) {
        let mut rates = Vec::new();
        let mut prev: Option<(u64, f64)> = None;
        for &(t, v) in history {
            if t < start {
                prev = Some((t, v));
                continue;
            }
            if t > now {
                break;
            }
            if let Some((pt, pv)) = prev {
                if t > pt {
                    let gap = t - pt;
                    if gap <= RATE_GAP_MS {
                        let dt = gap as f64 / 1000.0;
                        if dt > 0.0 {
                            rates.push(ChartPoint {
                                t: t_of(t),
                                value: (v - pv).max(0.0) / dt * 60.0,
                            });
                        }
                    }
                }
            }
            prev = Some((t, v));
        }
        rates
    } else {
        history
            .iter()
            .copied()
            .filter(|(t, _)| *t >= start && *t <= now)
            .map(|(t, value)| ChartPoint { t: t_of(t), value })
            .collect()
    };
    downsample(points)
}

/// Requests (and other counters) in `[now - window, now]`: last sample minus
/// the last sample at or before the window start. Resets count as the new total.
pub fn window_count(
    query: &str,
    history: &[(u64, f64)],
    window: ObserveWindow,
    now: u64,
) -> Option<f64> {
    if !is_counter_query(query) {
        return None;
    }
    let start = now.saturating_sub(window.duration_ms());
    let mut baseline: Option<f64> = None;
    let mut last: Option<f64> = None;
    for &(t, v) in history {
        if t <= start {
            baseline = Some(v);
            last = Some(v);
            continue;
        }
        if t > now {
            break;
        }
        if baseline.is_none() {
            baseline = Some(v);
        }
        last = Some(v);
    }
    let (start_v, end_v) = (baseline?, last?);
    Some(if end_v >= start_v {
        end_v - start_v
    } else {
        end_v
    })
}

fn plot_range(history: &[(u64, f64)], window: ObserveWindow, now: u64) -> (u64, u64) {
    let window_span = window.duration_ms().max(1);
    let window_start = now.saturating_sub(window_span);
    let first = history
        .iter()
        .map(|(t, _)| *t)
        .find(|t| *t >= window_start && *t <= now)
        .unwrap_or(now);
    let data_span = now.saturating_sub(first);
    let span = data_span.max(MIN_PLOT_MS).min(window_span).max(1);
    let start = now.saturating_sub(span);
    (start, span)
}

fn downsample(pts: Vec<ChartPoint>) -> Vec<ChartPoint> {
    if pts.len() <= CHART_POINTS {
        return pts;
    }
    let mut buckets = vec![(0.0f32, 0.0f64, 0u32); CHART_POINTS];
    for p in pts {
        let i = ((p.t * CHART_POINTS as f32).floor() as usize).min(CHART_POINTS - 1);
        buckets[i].0 += p.t;
        buckets[i].1 += p.value;
        buckets[i].2 += 1;
    }
    buckets
        .into_iter()
        .filter(|b| b.2 > 0)
        .map(|(t, v, n)| ChartPoint {
            t: t / n as f32,
            value: v / n as f64,
        })
        .collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_counter_query(query: &str) -> bool {
    let query = query.trim();
    matches!(
        query,
        "total_requests" | "requests" | "5xx" | "4xx" | "2xx" | "3xx"
    ) || query.ends_with("_total")
        || query.ends_with("_count")
}

fn truncate_label(query: &str) -> String {
    let chars: String = query.chars().take(24).collect();
    if query.chars().count() > 24 {
        format!("{chars}…")
    } else {
        chars
    }
}

pub fn format_chart_sample(query: &str, value: f64) -> String {
    let sample = format_sample(value);
    if is_counter_query(query) {
        format!("{sample}/min")
    } else {
        sample
    }
}

pub fn format_chart_age(t: f32, window: ObserveWindow) -> String {
    let ago_ms = (1.0 - t.clamp(0.0, 1.0)) as f64 * window.duration_ms() as f64;
    if ago_ms < 20_000.0 {
        return "now".into();
    }
    let mins = (ago_ms / 60_000.0).round().max(1.0) as u64;
    if mins < 60 {
        if mins == 1 {
            "1 min ago".into()
        } else {
            format!("{mins} min ago")
        }
    } else {
        let hours = mins / 60;
        let rem = mins % 60;
        if rem == 0 {
            if hours == 1 {
                "1 h ago".into()
            } else {
                format!("{hours} h ago")
            }
        } else {
            format!("{hours} h {rem} min ago")
        }
    }
}

pub fn nearest_chart_point(series: &[ChartPoint], t: f32) -> Option<ChartPoint> {
    series.iter().copied().min_by(|a, b| {
        (a.t - t)
            .abs()
            .partial_cmp(&(b.t - t).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
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
        ObserveSourceKind::Warmup => poll_warmup(config).await,
        ObserveSourceKind::Prometheus if is_warmup_host(&config.prometheus_url) => {
            poll_warmup(config).await
        }
        ObserveSourceKind::Prometheus => poll_prometheus(config).await,
        other => ObserveSnapshot {
            error: Some(format!(
                "{other:?} is not wired yet — use warmUP or Prometheus for this island widget"
            )),
            ..ObserveSnapshot::default()
        },
    }
}

async fn poll_warmup(config: &ObserveConfig) -> ObserveSnapshot {
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

    let url = format!("{base}{WARMUP_METRICS_PATH}");
    let mut request = client.get(&url);
    let token = metrics_token(config);
    if !token.is_empty() {
        request = request.bearer_auth(token);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            match response.text().await {
                Ok(body) => {
                    if status.as_u16() == 401 {
                        return ObserveSnapshot {
                            error: Some(
                                "warmUP /admin/metrics needs a bearer token (Settings or WARMUP_METRICS_TOKEN)"
                                    .into(),
                            ),
                            ..ObserveSnapshot::default()
                        };
                    }
                    if !status.is_success() {
                        return ObserveSnapshot {
                            error: Some(format!(
                                "warmUP {WARMUP_METRICS_PATH} returned HTTP {status}"
                            )),
                            ..ObserveSnapshot::default()
                        };
                    }
                    match parse_warmup_metrics(&body, &warmup_metrics_for(config)) {
                        Ok(snap) => snap,
                        Err(err) => ObserveSnapshot {
                            error: Some(err),
                            ..ObserveSnapshot::default()
                        },
                    }
                }
                Err(err) => ObserveSnapshot {
                    error: Some(err.to_string()),
                    ..ObserveSnapshot::default()
                },
            }
        }
        Err(err) => ObserveSnapshot {
            error: Some(format_http_error(err, "warmUP")),
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
                    Ok(values) => MetricReading::from_metric(metric, values, None),
                    Err(err) => MetricReading::from_metric(
                        metric,
                        Vec::new(),
                        Some(if !status.is_success() {
                            format!("HTTP {status}: {err}")
                        } else {
                            err
                        }),
                    ),
                },
                Err(err) => MetricReading::from_metric(metric, Vec::new(), Some(err.to_string())),
            }
        }
        Err(err) => MetricReading::from_metric(
            metric,
            Vec::new(),
            Some(format_http_error(err, "Prometheus")),
        ),
    }
}

async fn fetch_alerts(client: &Client, base: &str) -> Result<Vec<FiringAlert>, String> {
    let url = format!("{base}/api/v1/alerts");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format_http_error(e, "Prometheus"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Prometheus /api/v1/alerts returned HTTP {status}"));
    }
    parse_alerts(&body)
}

/// Metric names from the active source, used by Settings to pick pins.
pub async fn list_metric_names(config: &ObserveConfig) -> Result<Vec<String>, String> {
    if uses_warmup(config) {
        return Ok(warmup_metric_names());
    }
    if config.source != ObserveSourceKind::Prometheus {
        return Err("Metric browse is Prometheus-only for now".into());
    }
    let base = normalize_base_url(&config.prometheus_url)?;
    let client = client()?;
    let url = format!("{base}/api/v1/label/__name__/values");
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format_http_error(e, "Prometheus"))?;
    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("Prometheus label values returned HTTP {status}"));
    }
    parse_label_values(&body)
}

fn format_http_error(err: reqwest::Error, source: &str) -> String {
    if err.is_connect() {
        format!("Can't reach {source}: {err}")
    } else if err.is_timeout() {
        format!("{source} timed out")
    } else {
        err.to_string()
    }
}

fn warmup_metric_names() -> Vec<String> {
    vec![
        "total_requests".into(),
        "5xx".into(),
        "4xx".into(),
        "slow".into(),
        "errors".into(),
    ]
}

fn is_warmup_query(query: &str) -> bool {
    matches!(
        query.trim(),
        "total_requests" | "requests" | "5xx" | "4xx" | "2xx" | "3xx" | "slow" | "errors"
    )
}

fn warmup_metrics_for(config: &ObserveConfig) -> Vec<ObserveMetric> {
    let pinned: Vec<_> = config
        .metrics
        .iter()
        .filter(|m| is_warmup_query(&m.query))
        .cloned()
        .collect();
    if pinned.is_empty() {
        default_warmup_metrics()
    } else {
        pinned
    }
}

fn parse_warmup_metrics(body: &str, metrics: &[ObserveMetric]) -> Result<ObserveSnapshot, String> {
    let parsed: WarmupMetricsSnapshot = serde_json::from_str(body)
        .map_err(|e| format!("Unexpected warmUP metrics response: {e}"))?;
    Ok(snapshot_from_warmup(parsed, metrics))
}

fn snapshot_from_warmup(
    parsed: WarmupMetricsSnapshot,
    metrics: &[ObserveMetric],
) -> ObserveSnapshot {
    let readings = metrics
        .iter()
        .map(|metric| warmup_reading(&parsed, metric))
        .collect();
    let alerts = parsed
        .recent_error_requests
        .iter()
        .filter(|s| s.status >= 500)
        .take(MAX_ALERTS)
        .map(|s| FiringAlert {
            name: format!("{} {}", s.method, s.path).trim().to_string(),
            severity: "critical".into(),
            summary: format!("HTTP {}", s.status),
        })
        .collect();
    ObserveSnapshot {
        connected: true,
        error: None,
        metrics: readings,
        alerts,
    }
}

fn warmup_reading(parsed: &WarmupMetricsSnapshot, metric: &ObserveMetric) -> MetricReading {
    let query = metric.query.trim();
    let value = match query {
        "total_requests" | "requests" => Some(parsed.total_requests as f64),
        "5xx" | "4xx" | "3xx" | "2xx" => Some(bucket_count(&parsed.counters, query)),
        "slow" => Some(parsed.recent_slow_requests.len() as f64),
        "errors" => Some(parsed.recent_error_requests.len() as f64),
        _ => None,
    };
    match value {
        Some(value) => MetricReading::from_metric(
            metric,
            vec![SeriesValue {
                name: String::new(),
                value,
            }],
            None,
        ),
        None => {
            MetricReading::from_metric(metric, Vec::new(), Some("Not a warmUP metric key".into()))
        }
    }
}

fn bucket_count(counters: &[WarmupCounter], bucket: &str) -> f64 {
    counters
        .iter()
        .filter(|c| c.status_bucket.eq_ignore_ascii_case(bucket))
        .map(|c| c.count)
        .sum()
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
            if let Some(value) = data.result.into_scalar() {
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
            .into_vector()
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

#[derive(Debug, Default, Deserialize)]
struct WarmupMetricsSnapshot {
    #[serde(default, rename = "totalRequests")]
    total_requests: u64,
    #[serde(default)]
    counters: Vec<WarmupCounter>,
    #[serde(default, rename = "recentSlowRequests")]
    recent_slow_requests: Vec<WarmupSample>,
    #[serde(default, rename = "recentErrorRequests")]
    recent_error_requests: Vec<WarmupSample>,
}

#[derive(Debug, Deserialize)]
struct WarmupCounter {
    #[serde(default, rename = "statusBucket")]
    status_bucket: String,
    #[serde(default)]
    count: f64,
}

#[derive(Debug, Deserialize)]
struct WarmupSample {
    #[serde(default)]
    method: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    status: u16,
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

#[derive(Debug, Default, Deserialize)]
#[serde(untagged)]
enum PromResult {
    Vector(Vec<PromVectorSample>),
    Scalar(PromValue),
    #[default]
    Empty,
}

impl PromResult {
    fn into_vector(self) -> Vec<SeriesValue> {
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

    fn into_scalar(self) -> Option<f64> {
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
        assert_eq!(format_chart_sample("total_requests", 60.0), "60/min");
        assert_eq!(format_chart_sample("slow", 3.0), "3");
        assert_eq!(format_chart_age(1.0, ObserveWindow::ThirtyMinutes), "now");
        assert_eq!(
            format_chart_age(0.0, ObserveWindow::ThirtyMinutes),
            "30 min ago"
        );
        let pts = [
            ChartPoint { t: 0.1, value: 1.0 },
            ChartPoint { t: 0.8, value: 9.0 },
        ];
        assert_eq!(nearest_chart_point(&pts, 0.75).unwrap().value, 9.0);
    }

    #[test]
    fn source_enum_roundtrips() {
        let json = serde_json::to_string(&ObserveSourceKind::Prometheus).unwrap();
        assert_eq!(json, "\"prometheus\"");
        let warmup = serde_json::to_string(&ObserveSourceKind::Warmup).unwrap();
        assert_eq!(warmup, "\"warmup\"");
        let _: ObserveSourceKind = serde_json::from_str("\"grafana\"").unwrap();
        let _: ObserveSourceKind = serde_json::from_str("\"alertmanager\"").unwrap();
        let _: ObserveSourceKind = serde_json::from_str("\"fm_observe\"").unwrap();
        let _: ObserveSourceKind = serde_json::from_str("\"warmup\"").unwrap();
    }

    #[test]
    fn chart_kind_defaults_and_cycles() {
        let parsed: ObserveMetric = serde_json::from_str(r#"{"label":"Up","query":"up"}"#).unwrap();
        assert_eq!(parsed.chart, ObserveChartKind::Sparkline);
        assert_eq!(ObserveChartKind::Sparkline.next(), ObserveChartKind::Bars);
        assert_eq!(ObserveChartKind::Bars.next(), ObserveChartKind::Off);
        assert_eq!(ObserveChartKind::Off.next(), ObserveChartKind::Sparkline);
        let mut config = ObserveConfig::default();
        cycle_metric_chart(&mut config, "5xx");
        assert_eq!(
            config
                .metrics
                .iter()
                .find(|m| m.query == "5xx")
                .unwrap()
                .chart,
            ObserveChartKind::Bars
        );
        assert_eq!(config.window, ObserveWindow::ThirtyMinutes);
        assert_eq!(
            ObserveWindow::ThirtyMinutes.next(),
            ObserveWindow::FiveHours
        );
        assert_eq!(ObserveWindow::FiveHours.next(), ObserveWindow::OneDay);
        assert_eq!(ObserveWindow::OneDay.next(), ObserveWindow::ThirtyMinutes);
    }

    #[test]
    fn history_windows_counter_rate_on_the_right() {
        let mut store = HashMap::new();
        let mut snap = ObserveSnapshot {
            metrics: vec![MetricReading::from_metric(
                &ObserveMetric::new("Requests", "total_requests"),
                vec![SeriesValue {
                    name: String::new(),
                    value: 10.0,
                }],
                None,
            )],
            ..ObserveSnapshot::default()
        };
        let now = 10_000_000;
        record_history_at(
            &mut store,
            &mut snap,
            ObserveWindow::ThirtyMinutes,
            now - 15_000,
            false,
        );
        assert!(snap.metrics[0].history.is_empty());
        snap.metrics[0].values[0].value = 25.0;
        record_history_at(
            &mut store,
            &mut snap,
            ObserveWindow::ThirtyMinutes,
            now,
            false,
        );
        assert_eq!(snap.metrics[0].history.len(), 1);
        let pt = snap.metrics[0].history[0];
        assert!(
            (pt.t - 1.0).abs() < 0.02,
            "recent sample sits at now, t={}",
            pt.t
        );
        assert!(
            (pt.value - 60.0).abs() < 0.01,
            "15 events / 15s = 60/min, got {}",
            pt.value
        );

        let later = now + DAY_MS + STORE_SLACK_MS + 1;
        snap.metrics[0].values[0].value = 40.0;
        record_history_at(
            &mut store,
            &mut snap,
            ObserveWindow::ThirtyMinutes,
            later,
            false,
        );
        assert!(store["total_requests"]
            .iter()
            .all(|(t, _)| *t >= later - DAY_MS - STORE_SLACK_MS));
    }

    #[test]
    fn sqlite_keeps_a_day_of_samples() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(SAMPLES_TABLE, []).unwrap();
        let now = DAY_MS + 200_000;
        write_samples(
            &conn,
            &[
                ("slow".into(), 1, 1.0),
                ("slow".into(), now - 3_600_000, 4.0),
                ("slow".into(), now, 9.0),
            ],
            now.saturating_sub(DAY_MS + STORE_SLACK_MS),
        )
        .unwrap();
        let loaded = load_history_from(&conn, now);
        assert_eq!(loaded["slow"].len(), 2);
        assert_eq!(loaded["slow"][1], (now, 9.0));
    }

    #[test]
    fn window_filters_old_gauge_samples() {
        let now = 40 * 60 * 1000;
        let history = vec![(0, 3.0), (now - 10 * 60 * 1000, 4.0), (now, 9.0)];
        let pts = chart_values("slow", &history, ObserveWindow::ThirtyMinutes, now);
        assert_eq!(pts.len(), 2);
        assert!((pts.last().unwrap().value - 9.0).abs() < 1e-9);
        assert!(pts.last().unwrap().t > 0.9);
        assert!(pts[0].t < 0.15);
    }

    #[test]
    fn requests_sum_follows_the_visible_window() {
        let now = 6 * 60 * 60 * 1000;
        let history = vec![
            (now - 6 * 60 * 60 * 1000, 0.0),
            (now - 30 * 60 * 1000, 100.0),
            (now, 110.0),
        ];
        assert_eq!(
            window_count(
                "total_requests",
                &history,
                ObserveWindow::ThirtyMinutes,
                now
            ),
            Some(10.0)
        );
        assert_eq!(
            window_count("total_requests", &history, ObserveWindow::FiveHours, now),
            Some(110.0)
        );
        assert_eq!(
            window_count("total_requests", &history, ObserveWindow::OneDay, now),
            Some(110.0)
        );
        assert_eq!(
            window_count("slow", &history, ObserveWindow::OneDay, now),
            None
        );
    }

    #[test]
    fn short_history_fills_the_plot_instead_of_a_right_sliver() {
        let now = 30 * 60 * 1000;
        let history = vec![(now - 90_000, 10.0), (now - 45_000, 20.0), (now, 30.0)];
        let pts = chart_values("slow", &history, ObserveWindow::ThirtyMinutes, now);
        assert_eq!(pts.len(), 3);
        assert!(
            pts[0].t < 0.4,
            "90s of data in a 30 min window should still span the plot, t={}",
            pts[0].t
        );
        assert!(pts.last().unwrap().t > 0.9);
    }

    #[test]
    fn default_points_at_warmup_api() {
        let config = ObserveConfig::default();
        assert_eq!(config.source, ObserveSourceKind::Warmup);
        assert_eq!(config.prometheus_url, DEFAULT_OBSERVE_URL);
        let parsed: ObserveConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, config);
        let mut empty = ObserveConfig {
            prometheus_url: String::new(),
            ..ObserveConfig::default()
        };
        fill_default_url(&mut empty);
        assert_eq!(empty.prometheus_url, DEFAULT_OBSERVE_URL);
    }

    #[test]
    fn parses_warmup_admin_metrics() {
        let body = r#"{
            "totalRequests": 12,
            "slowThresholdMs": 10000,
            "counters": [
                {"key":"GET /health 2xx","method":"GET","path":"/health","statusBucket":"2xx","count":10,"totalDurationMs":12,"maxDurationMs":3},
                {"key":"GET /v1/x 5xx","method":"GET","path":"/v1/x","statusBucket":"5xx","count":2,"totalDurationMs":40,"maxDurationMs":30}
            ],
            "recentSlowRequests": [{"method":"GET","path":"/slow","status":200,"statusBucket":"2xx","durationMs":12000,"requestId":"a","recordedAt":"2026-01-01T00:00:00Z"}],
            "recentErrorRequests": [
                {"method":"GET","path":"/v1/x","status":500,"statusBucket":"5xx","durationMs":20,"requestId":"b","recordedAt":"2026-01-01T00:00:00Z"},
                {"method":"POST","path":"/v1/chat","status":401,"statusBucket":"4xx","durationMs":1,"requestId":"c","recordedAt":"2026-01-01T00:00:00Z"}
            ],
            "rateLimits": {}
        }"#;
        let snap = parse_warmup_metrics(body, &default_warmup_metrics()).unwrap();
        assert!(snap.connected);
        assert_eq!(snap.metrics[0].values[0].value, 12.0);
        assert_eq!(snap.metrics[1].values[0].value, 2.0);
        assert_eq!(snap.metrics[2].values[0].value, 1.0);
        assert_eq!(snap.alerts.len(), 1);
        assert_eq!(snap.alerts[0].name, "GET /v1/x");
        assert_eq!(snap.alerts[0].summary, "HTTP 500");
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
            source: ObserveSourceKind::Prometheus,
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

    #[tokio::test]
    async fn polls_warmup_admin_metrics() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                let mut stream = incoming.unwrap();
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let body = if req.contains(WARMUP_METRICS_PATH) {
                    r#"{"totalRequests":7,"counters":[{"statusBucket":"5xx","count":1}],"recentSlowRequests":[],"recentErrorRequests":[{"method":"GET","path":"/v1/x","status":503}]}"#
                } else {
                    r#"{"error":"missing"}"#
                };
                let status = if req.contains(WARMUP_METRICS_PATH) {
                    "200 OK"
                } else {
                    "404 Not Found"
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });

        let config = ObserveConfig {
            source: ObserveSourceKind::Warmup,
            prometheus_url: format!("http://{addr}"),
            metrics: default_warmup_metrics(),
            ..ObserveConfig::default()
        };
        let snap = poll(&config).await;
        assert!(
            snap.connected,
            "expected warmup snapshot, got {:?}",
            snap.error
        );
        assert_eq!(snap.metrics[0].values[0].value, 7.0);
        assert_eq!(snap.alerts[0].name, "GET /v1/x");
    }
}
