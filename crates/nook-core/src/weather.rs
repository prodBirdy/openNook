//! Open-Meteo weather client.
//!
//! Keyless forecast + geocoding. Snapshots are cached for [`CACHE_TTL`]
//! (30 min). Callers must not poll faster than that — [`fetch`] returns the
//! cached snapshot when it is still fresh, and network failures back off
//! exponentially up to the same cap.
//!
//! The location is always the Mac's current one (CoreLocation via
//! [`crate::location`]), re-checked at most every [`LOCATION_REFRESH`]. A fix
//! only replaces the stored coords when it moved more than
//! [`LOCATION_MOVE_KM`], so a jittery fix does not refetch the forecast.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Open-Meteo model updates hourly; anything faster is wasted radio.
pub const CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BACKOFF: Duration = CACHE_TTL;
const HOURLY_POINTS: usize = 8;
const USER_AGENT: &str = "openNook (https://github.com/prodBirdy/openNook)";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODE_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// Shortest gap between two CoreLocation requests.
pub const LOCATION_REFRESH: Duration = Duration::from_secs(15 * 60);
/// A new fix closer than this to the stored one keeps the stored coords.
pub const LOCATION_MOVE_KM: f64 = 1.0;
/// Card / Settings label when the fix has no reverse-geocoded city yet.
pub const CURRENT_LOCATION: &str = "Current location";

/// CC-BY 4.0 attribution required by Open-Meteo's free tier.
pub const ATTRIBUTION: &str = "Weather data by Open-Meteo.com";

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WeatherUnits {
    #[default]
    Celsius,
    Fahrenheit,
}

impl WeatherUnits {
    pub fn query_value(self) -> &'static str {
        match self {
            Self::Celsius => "celsius",
            Self::Fahrenheit => "fahrenheit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
        }
    }
}

/// Where the forecast is for. `Manual` only survives as a stored format from
/// older builds: [`WeatherSettings`] turns it into `System` on load, keeping
/// its coords until the first real fix replaces them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum WeatherLocationMode {
    Manual { name: String, lat: f64, lon: f64 },
    System { name: String, lat: f64, lon: f64 },
}

impl Default for WeatherLocationMode {
    fn default() -> Self {
        Self::System {
            name: String::new(),
            lat: 0.0,
            lon: 0.0,
        }
    }
}

impl WeatherLocationMode {
    /// Always the current location; a stored manual city becomes its seed.
    pub fn into_system(self) -> Self {
        match self {
            Self::Manual { name, lat, lon } | Self::System { name, lat, lon } => {
                Self::System { name, lat, lon }
            }
        }
    }

    /// City for display, or [`CURRENT_LOCATION`] when the fix has no name.
    pub fn display_name(&self) -> &str {
        let name = self.name();
        if name.trim().is_empty() {
            CURRENT_LOCATION
        } else {
            name
        }
    }

    pub fn is_system(&self) -> bool {
        matches!(self, Self::System { .. })
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Manual { name, .. } | Self::System { name, .. } => name,
        }
    }

    pub fn coords(&self) -> Option<(f64, f64)> {
        let (lat, lon) = match self {
            Self::Manual { lat, lon, .. } | Self::System { lat, lon, .. } => (*lat, *lon),
        };
        if lat == 0.0 && lon == 0.0 {
            None
        } else {
            Some((lat, lon))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeatherSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub units: WeatherUnits,
    #[serde(default, deserialize_with = "system_location")]
    pub location: WeatherLocationMode,
    #[serde(default = "default_true")]
    pub show_on_compact_face: bool,
}

fn system_location<'de, D>(deserializer: D) -> Result<WeatherLocationMode, D::Error>
where
    D: serde::Deserializer<'de>,
{
    WeatherLocationMode::deserialize(deserializer).map(WeatherLocationMode::into_system)
}

impl Default for WeatherSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            units: WeatherUnits::Celsius,
            location: WeatherLocationMode::default(),
            show_on_compact_face: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HourlyForecast {
    pub hour: String,
    pub temperature: f64,
    pub wmo_code: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WeatherSnapshot {
    pub location_name: String,
    pub latitude: f64,
    pub longitude: f64,
    pub units: WeatherUnits,
    pub temperature: f64,
    pub feels_like: f64,
    pub wmo_code: u8,
    pub is_day: bool,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub precip_probability: Option<u8>,
    pub wind_speed: Option<f64>,
    /// Relative humidity % from Open-Meteo `current.relative_humidity_2m`.
    pub humidity: Option<u8>,
    /// UV index (rounded) from `current.uv_index` or daily `uv_index_max`.
    pub uv_index: Option<u8>,
    pub hourly: Vec<HourlyForecast>,
    pub fetched_at: Instant,
}

impl WeatherSnapshot {
    pub fn is_fresh(&self) -> bool {
        snapshot_is_fresh(self.fetched_at, CACHE_TTL)
    }

    pub fn matches(&self, lat: f64, lon: f64, units: WeatherUnits) -> bool {
        self.units == units
            && (self.latitude - lat).abs() < 1e-4
            && (self.longitude - lon).abs() < 1e-4
    }

    pub fn icon(&self) -> &'static str {
        wmo_icon(self.wmo_code, self.is_day)
    }

    pub fn label(&self) -> &'static str {
        wmo_label(self.wmo_code)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeoPlace {
    pub name: String,
    pub country: Option<String>,
    pub admin1: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
}

impl GeoPlace {
    pub fn display_name(&self) -> String {
        match (&self.admin1, &self.country) {
            (Some(admin), Some(country)) if !admin.is_empty() => {
                format!("{}, {}, {}", self.name, admin, country)
            }
            (_, Some(country)) => format!("{}, {}", self.name, country),
            _ => self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedForecast {
    pub temperature: f64,
    pub feels_like: f64,
    pub wmo_code: u8,
    pub is_day: bool,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub precip_probability: Option<u8>,
    pub wind_speed: Option<f64>,
    pub humidity: Option<u8>,
    pub uv_index: Option<u8>,
    pub hourly: Vec<HourlyForecast>,
}

#[derive(Deserialize)]
struct ForecastResponse {
    current: Option<CurrentBlock>,
    daily: Option<DailyBlock>,
    hourly: Option<HourlyBlock>,
}

#[derive(Deserialize)]
struct CurrentBlock {
    time: Option<String>,
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    weather_code: Option<u8>,
    wind_speed_10m: Option<f64>,
    is_day: Option<u8>,
    #[serde(default)]
    relative_humidity_2m: Option<f64>,
    #[serde(default)]
    uv_index: Option<f64>,
}

#[derive(Deserialize)]
struct DailyBlock {
    temperature_2m_max: Option<Vec<Option<f64>>>,
    temperature_2m_min: Option<Vec<Option<f64>>>,
    precipitation_probability_max: Option<Vec<Option<i32>>>,
    #[serde(default)]
    uv_index_max: Option<Vec<Option<f64>>>,
}

#[derive(Deserialize)]
struct HourlyBlock {
    time: Option<Vec<String>>,
    temperature_2m: Option<Vec<Option<f64>>>,
    weather_code: Option<Vec<Option<u8>>>,
}

#[derive(Deserialize)]
struct GeocodeResponse {
    results: Option<Vec<GeocodeHit>>,
}

#[derive(Deserialize)]
struct GeocodeHit {
    name: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    country: Option<String>,
    admin1: Option<String>,
}

struct Cache {
    snapshot: Option<WeatherSnapshot>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache { snapshot: None }))
}

fn fail_streak() -> &'static AtomicU32 {
    static STREAK: AtomicU32 = AtomicU32::new(0);
    &STREAK
}

fn next_attempt() -> &'static Mutex<Option<Instant>> {
    static NEXT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    NEXT.get_or_init(|| Mutex::new(None))
}

fn fetching() -> &'static AtomicBool {
    static FETCHING: AtomicBool = AtomicBool::new(false);
    &FETCHING
}

fn wake_flag() -> &'static AtomicBool {
    static WAKE: AtomicBool = AtomicBool::new(false);
    &WAKE
}

/// True when `fetched_at` is still inside `ttl`.
pub fn snapshot_is_fresh(fetched_at: Instant, ttl: Duration) -> bool {
    fetched_at.elapsed() < ttl
}

/// Format a temperature for the compact face / card (`18°`).
pub fn format_temp(value: f64) -> String {
    format!("{:.0}°", value.round())
}

/// Lucide icon name for a WMO weather code, with a night variant when `is_day`
/// is false.
pub fn wmo_icon(code: u8, is_day: bool) -> &'static str {
    match code {
        0 => {
            if is_day {
                "sun"
            } else {
                "moon"
            }
        }
        1 | 2 => {
            if is_day {
                "cloud-sun"
            } else {
                "cloud-moon"
            }
        }
        3 => "cloud",
        45 | 48 => "cloud-fog",
        51 | 53 | 55 | 56 | 57 => "cloud-drizzle",
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => "cloud-rain",
        71 | 73 | 75 | 77 | 85 | 86 => "cloud-snow",
        95 | 96 | 99 => "cloud-lightning",
        _ => {
            if is_day {
                "cloud-sun"
            } else {
                "cloud-moon"
            }
        }
    }
}

/// Short English label for a WMO weather code.
pub fn wmo_label(code: u8) -> &'static str {
    match code {
        0 => "Clear",
        1 => "Mostly clear",
        2 => "Partly cloudy",
        3 => "Overcast",
        45 => "Fog",
        48 => "Rime fog",
        51 => "Light drizzle",
        53 => "Drizzle",
        55 => "Heavy drizzle",
        56 => "Freezing drizzle",
        57 => "Heavy freezing drizzle",
        61 => "Light rain",
        63 => "Rain",
        65 => "Heavy rain",
        66 => "Freezing rain",
        67 => "Heavy freezing rain",
        71 => "Light snow",
        73 => "Snow",
        75 => "Heavy snow",
        77 => "Snow grains",
        80 => "Light showers",
        81 => "Showers",
        82 => "Heavy showers",
        85 => "Light snow showers",
        86 => "Snow showers",
        95 => "Thunderstorm",
        96 => "Thunderstorm + hail",
        99 => "Severe thunderstorm",
        _ => "Unknown",
    }
}

/// Parse an Open-Meteo forecast body. Location / units / fetched_at are filled
/// by [`fetch`].
pub fn parse_forecast(json: &str) -> Result<ParsedForecast, String> {
    let parsed: ForecastResponse =
        serde_json::from_str(json).map_err(|err| format!("weather parse: {err}"))?;
    let current = parsed
        .current
        .ok_or_else(|| "weather parse: missing current".to_string())?;
    let temperature = current
        .temperature_2m
        .ok_or_else(|| "weather parse: missing temperature".to_string())?;
    let feels_like = current.apparent_temperature.unwrap_or(temperature);
    let wmo_code = current.weather_code.unwrap_or(0);
    let is_day = current.is_day.unwrap_or(1) != 0;
    let wind_speed = current.wind_speed_10m;
    let humidity = current
        .relative_humidity_2m
        .map(|h| h.round().clamp(0.0, 100.0) as u8);
    let (high, low, precip_probability, daily_uv) = match parsed.daily {
        Some(daily) => (
            first_opt_f64(daily.temperature_2m_max.as_deref()),
            first_opt_f64(daily.temperature_2m_min.as_deref()),
            first_opt_u8(daily.precipitation_probability_max.as_deref()),
            first_opt_f64(daily.uv_index_max.as_deref()),
        ),
        None => (None, None, None, None),
    };
    let uv_index = current
        .uv_index
        .or(daily_uv)
        .map(|u| u.round().clamp(0.0, 20.0) as u8);
    let hourly = parsed
        .hourly
        .map(|block| take_hourly(&block, current.time.as_deref(), HOURLY_POINTS))
        .unwrap_or_default();
    Ok(ParsedForecast {
        temperature,
        feels_like,
        wmo_code,
        is_day,
        high,
        low,
        precip_probability,
        wind_speed,
        humidity,
        uv_index,
        hourly,
    })
}

fn first_opt_f64(values: Option<&[Option<f64>]>) -> Option<f64> {
    values.and_then(|list| list.iter().flatten().copied().next())
}

fn first_opt_u8(values: Option<&[Option<i32>]>) -> Option<u8> {
    values.and_then(|list| {
        list.iter()
            .flatten()
            .find_map(|v| u8::try_from(*v).ok().or(Some((*v).clamp(0, 100) as u8)))
    })
}

fn take_hourly(block: &HourlyBlock, from_time: Option<&str>, limit: usize) -> Vec<HourlyForecast> {
    let times = block.time.as_deref().unwrap_or(&[]);
    let temps = block.temperature_2m.as_deref().unwrap_or(&[]);
    let codes = block.weather_code.as_deref().unwrap_or(&[]);
    let start = from_time
        .and_then(|from| times.iter().position(|t| t.as_str() >= from))
        .unwrap_or(0);
    times
        .iter()
        .zip(temps.iter().chain(std::iter::repeat(&None)))
        .zip(codes.iter().chain(std::iter::repeat(&None)))
        .skip(start)
        .filter_map(|((time, temp), code)| {
            Some(HourlyForecast {
                hour: hour_label(time),
                temperature: (*temp)?,
                wmo_code: code.unwrap_or(0),
            })
        })
        .take(limit)
        .collect()
}

fn hour_label(time: &str) -> String {
    time.split('T')
        .nth(1)
        .and_then(|clock| clock.get(..2))
        .unwrap_or("--")
        .to_string()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|err| err.to_string())
}

/// Cached snapshot when one exists, regardless of age.
pub fn cached_snapshot() -> Option<WeatherSnapshot> {
    cache().lock().ok().and_then(|guard| guard.snapshot.clone())
}

/// Cached snapshot only when it still matches `settings` and is within TTL.
pub fn fresh_snapshot(settings: &WeatherSettings) -> Option<WeatherSnapshot> {
    let (lat, lon) = settings.location.coords()?;
    let guard = cache().lock().ok()?;
    let snap = guard.snapshot.as_ref()?;
    if snap.matches(lat, lon, settings.units) && snap.is_fresh() {
        Some(snap.clone())
    } else {
        None
    }
}

pub fn is_fresh_for(settings: &WeatherSettings) -> bool {
    fresh_snapshot(settings).is_some()
}

/// Drop the in-memory cache (location / units changed).
pub fn invalidate() {
    if let Ok(mut guard) = cache().lock() {
        guard.snapshot = None;
    }
    fail_streak().store(0, Ordering::Relaxed);
    if let Ok(mut next) = next_attempt().lock() {
        *next = None;
    }
}

/// Wake notification from `NSWorkspaceDidWakeNotification`.
pub fn note_wake() {
    wake_flag().store(true, Ordering::Relaxed);
}

pub fn take_wake() -> bool {
    wake_flag().swap(false, Ordering::Relaxed)
}

fn backoff_allows() -> bool {
    let Ok(next) = next_attempt().lock() else {
        return true;
    };
    match *next {
        Some(when) => Instant::now() >= when,
        None => true,
    }
}

fn note_success() {
    fail_streak().store(0, Ordering::Relaxed);
    if let Ok(mut next) = next_attempt().lock() {
        *next = None;
    }
}

fn note_failure() {
    let streak = fail_streak().fetch_add(1, Ordering::Relaxed) + 1;
    let secs = 30u64.saturating_mul(1u64 << streak.min(6).saturating_sub(1).min(6));
    let wait = Duration::from_secs(secs).min(MAX_BACKOFF);
    if let Ok(mut next) = next_attempt().lock() {
        *next = Some(Instant::now() + wait);
    }
}

#[derive(Default)]
struct LocationState {
    last_request: Option<Instant>,
    inflight: bool,
    /// A real fix landed this run; until then the stored (maybe migrated
    /// manual) coords are only a seed and the first fix always replaces them.
    has_fix: bool,
    error: Option<String>,
}

fn location_state() -> &'static Mutex<LocationState> {
    static STATE: OnceLock<Mutex<LocationState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(LocationState::default()))
}

/// Pure throttle: a new request is due on first run or after [`LOCATION_REFRESH`].
pub fn location_request_due(last_request: Option<Instant>, now: Instant) -> bool {
    last_request.is_none_or(|at| now.duration_since(at) >= LOCATION_REFRESH)
}

/// Great-circle distance in km (haversine; plenty for a 1 km threshold).
pub fn distance_km((lat1, lon1): (f64, f64), (lat2, lon2): (f64, f64)) -> f64 {
    const EARTH_KM: f64 = 6371.0;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_KM * a.sqrt().min(1.0).asin()
}

/// Pure: whether `fix` should replace the stored coords. The first fix of a
/// run always does (the stored value may be a migrated manual city).
pub fn location_moved(stored: Option<(f64, f64)>, fix: (f64, f64), first_fix: bool) -> bool {
    match stored {
        _ if first_fix => true,
        None => true,
        Some(stored) => distance_km(stored, fix) > LOCATION_MOVE_KM,
    }
}

/// Claim the next CoreLocation request. False while one is in flight or the
/// last one is younger than [`LOCATION_REFRESH`]; true stamps the attempt, and
/// the caller must hand the result to [`finish_location_request`].
pub fn begin_location_request() -> bool {
    let Ok(mut state) = location_state().lock() else {
        return false;
    };
    let now = Instant::now();
    if state.inflight || !location_request_due(state.last_request, now) {
        return false;
    }
    state.inflight = true;
    state.last_request = Some(now);
    true
}

/// Record a location result. On a fix that moved (or the first fix), stores
/// it as the weather location, drops the forecast cache, and returns the
/// coords so the caller can reverse-geocode them and refetch.
pub fn finish_location_request(result: Result<(f64, f64), String>) -> Option<(f64, f64)> {
    let Ok(mut state) = location_state().lock() else {
        return None;
    };
    state.inflight = false;
    let fix = match result {
        Ok(fix) => fix,
        Err(err) => {
            state.error = Some(err);
            return None;
        }
    };
    state.error = None;
    let first_fix = !state.has_fix;
    state.has_fix = true;
    drop(state);
    let stored = crate::settings::get_app_settings().weather.location.coords();
    if !location_moved(stored, fix, first_fix) {
        return None;
    }
    let (lat, lon) = fix;
    crate::settings::tweak_app_settings(|s| {
        // A reverse-geocoded name follows; keep the old one only when we
        // barely moved (first fix over a nearby migrated city).
        let keep = stored.is_some_and(|old| distance_km(old, fix) <= LOCATION_MOVE_KM);
        let name = if keep {
            s.weather.location.name().to_string()
        } else {
            String::new()
        };
        s.weather.location = WeatherLocationMode::System { name, lat, lon };
    });
    invalidate();
    Some(fix)
}

/// Store the reverse-geocoded city for `fix`, if the location has not moved on.
pub fn set_location_name(fix: (f64, f64), name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    crate::settings::tweak_app_settings(|s| {
        if s.weather.location.coords() == Some(fix) {
            s.weather.location = WeatherLocationMode::System {
                name: name.clone(),
                lat: fix.0,
                lon: fix.1,
            };
        }
    });
    if let Ok(mut guard) = cache().lock() {
        if let Some(snap) = guard.snapshot.as_mut() {
            if snap.latitude == fix.0 && snap.longitude == fix.1 {
                snap.location_name = name;
            }
        }
    }
}

/// Last location failure (denied, no fix, …) for the Settings hint, if the
/// most recent request failed.
pub fn location_error() -> Option<String> {
    location_state().lock().ok().and_then(|s| s.error.clone())
}

/// Whether a real fix has landed since launch.
pub fn has_location_fix() -> bool {
    location_state().lock().is_ok_and(|s| s.has_fix)
}

/// TTL-gated fetch. Returns the cached snapshot when it is still fresh for
/// this location and unit system. Concurrent callers share one in-flight GET.
pub async fn fetch(settings: &WeatherSettings) -> Result<WeatherSnapshot, String> {
    let (lat, lon) = settings
        .location
        .coords()
        .ok_or_else(|| "Waiting for your location".to_string())?;
    if let Some(snap) = fresh_snapshot(settings) {
        return Ok(snap);
    }
    if !backoff_allows() {
        return cached_snapshot()
            .filter(|snap| snap.matches(lat, lon, settings.units))
            .ok_or_else(|| "Weather is temporarily unavailable".to_string());
    }
    if fetching()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return cached_snapshot()
            .filter(|snap| snap.matches(lat, lon, settings.units))
            .ok_or_else(|| "Weather is updating".to_string());
    }
    let result = fetch_uncached(lat, lon, settings).await;
    fetching().store(false, Ordering::SeqCst);
    match result {
        Ok(snap) => {
            note_success();
            if let Ok(mut guard) = cache().lock() {
                guard.snapshot = Some(snap.clone());
            }
            Ok(snap)
        }
        Err(err) => {
            note_failure();
            if let Some(snap) = cached_snapshot().filter(|s| s.matches(lat, lon, settings.units)) {
                Ok(snap)
            } else {
                Err(err)
            }
        }
    }
}

async fn fetch_uncached(
    lat: f64,
    lon: f64,
    settings: &WeatherSettings,
) -> Result<WeatherSnapshot, String> {
    let client = client()?;
    let url = format!(
        "{FORECAST_URL}?latitude={lat:.4}&longitude={lon:.4}\
         &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m,is_day,relative_humidity_2m,uv_index\
         &daily=temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max,uv_index_max\
         &hourly=temperature_2m,weather_code\
         &forecast_days=5&temperature_unit={}",
        settings.units.query_value()
    );
    let body = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("weather: {err}"))?
        .error_for_status()
        .map_err(|err| format!("weather: {err}"))?
        .text()
        .await
        .map_err(|err| format!("weather: {err}"))?;
    let parsed = parse_forecast(&body)?;
    Ok(WeatherSnapshot {
        location_name: settings.location.display_name().to_string(),
        latitude: lat,
        longitude: lon,
        units: settings.units,
        temperature: parsed.temperature,
        feels_like: parsed.feels_like,
        wmo_code: parsed.wmo_code,
        is_day: parsed.is_day,
        high: parsed.high,
        low: parsed.low,
        precip_probability: parsed.precip_probability,
        wind_speed: parsed.wind_speed,
        humidity: parsed.humidity,
        uv_index: parsed.uv_index,
        hourly: parsed.hourly,
        fetched_at: Instant::now(),
    })
}

/// Search Open-Meteo's geocoder. Returns up to `count` places (max 5).
pub async fn search_places(name: &str, count: usize) -> Result<Vec<GeoPlace>, String> {
    let query = name.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let count = count.clamp(1, 5);
    let client = client()?;
    let url = format!(
        "{GEOCODE_URL}?name={}&count={count}&language=en&format=json",
        urlencoding_name(query)
    );
    let body = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("geocode: {err}"))?
        .error_for_status()
        .map_err(|err| format!("geocode: {err}"))?
        .text()
        .await
        .map_err(|err| format!("geocode: {err}"))?;
    parse_geocode(&body)
}

fn urlencoding_name(name: &str) -> String {
    let mut out = String::new();
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

pub fn parse_geocode(json: &str) -> Result<Vec<GeoPlace>, String> {
    let parsed: GeocodeResponse =
        serde_json::from_str(json).map_err(|err| format!("geocode parse: {err}"))?;
    Ok(parsed
        .results
        .unwrap_or_default()
        .into_iter()
        .filter_map(|hit| {
            Some(GeoPlace {
                name: hit.name?,
                country: hit.country,
                admin1: hit.admin1,
                latitude: hit.latitude?,
                longitude: hit.longitude?,
            })
        })
        .take(5)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "current": {
            "time": "2026-08-28T14:00",
            "temperature_2m": 18.4,
            "apparent_temperature": 17.1,
            "weather_code": 2,
            "wind_speed_10m": 8.2,
            "is_day": 1,
            "relative_humidity_2m": 64.0,
            "uv_index": 3.2
        },
        "daily": {
            "time": ["2026-08-28"],
            "temperature_2m_max": [22.0],
            "temperature_2m_min": [11.5],
            "weather_code": [2],
            "precipitation_probability_max": [20],
            "uv_index_max": [5.0]
        },
        "hourly": {
            "time": [
                "2026-08-28T12:00",
                "2026-08-28T13:00",
                "2026-08-28T14:00",
                "2026-08-28T15:00",
                "2026-08-28T16:00"
            ],
            "temperature_2m": [16.0, 17.0, 18.4, 19.0, 19.5],
            "weather_code": [1, 1, 2, 2, 3]
        }
    }"#;

    #[test]
    fn wmo_maps_clear_day_and_night() {
        assert_eq!(wmo_icon(0, true), "sun");
        assert_eq!(wmo_icon(0, false), "moon");
        assert_eq!(wmo_label(0), "Clear");
    }

    #[test]
    fn wmo_maps_documented_codes() {
        assert_eq!(wmo_icon(3, true), "cloud");
        assert_eq!(wmo_label(3), "Overcast");
        assert_eq!(wmo_icon(45, true), "cloud-fog");
        assert_eq!(wmo_icon(61, true), "cloud-rain");
        assert_eq!(wmo_label(61), "Light rain");
        assert_eq!(wmo_icon(73, true), "cloud-snow");
        assert_eq!(wmo_icon(95, false), "cloud-lightning");
        assert_eq!(wmo_label(95), "Thunderstorm");
        assert_eq!(wmo_icon(2, false), "cloud-moon");
        assert_eq!(wmo_label(99), "Severe thunderstorm");
        assert_eq!(wmo_label(200), "Unknown");
    }

    #[test]
    fn parse_forecast_reads_current_daily_and_hourly() {
        let parsed = parse_forecast(SAMPLE).unwrap();
        assert!((parsed.temperature - 18.4).abs() < f64::EPSILON);
        assert!((parsed.feels_like - 17.1).abs() < f64::EPSILON);
        assert_eq!(parsed.wmo_code, 2);
        assert!(parsed.is_day);
        assert_eq!(parsed.high, Some(22.0));
        assert_eq!(parsed.low, Some(11.5));
        assert_eq!(parsed.precip_probability, Some(20));
        assert_eq!(parsed.wind_speed, Some(8.2));
        assert_eq!(parsed.humidity, Some(64));
        assert_eq!(parsed.uv_index, Some(3));
        assert_eq!(parsed.hourly.len(), 3);
        assert_eq!(parsed.hourly[0].hour, "14");
        assert!((parsed.hourly[0].temperature - 18.4).abs() < f64::EPSILON);
        assert_eq!(parsed.hourly[0].wmo_code, 2);
        assert_eq!(parsed.hourly[2].hour, "16");
    }

    #[test]
    fn parse_forecast_humidity_and_uv_default_when_absent() {
        let json = r#"{
            "current": {
                "time": "2026-08-28T14:00",
                "temperature_2m": 18.0,
                "weather_code": 0,
                "is_day": 1
            }
        }"#;
        let parsed = parse_forecast(json).unwrap();
        assert_eq!(parsed.humidity, None);
        assert_eq!(parsed.uv_index, None);
    }

    #[test]
    fn parse_forecast_falls_back_to_daily_uv_max() {
        let json = r#"{
            "current": {
                "time": "2026-08-28T14:00",
                "temperature_2m": 18.0,
                "weather_code": 0,
                "is_day": 1,
                "relative_humidity_2m": 55
            },
            "daily": {
                "uv_index_max": [8.6]
            }
        }"#;
        let parsed = parse_forecast(json).unwrap();
        assert_eq!(parsed.humidity, Some(55));
        assert_eq!(parsed.uv_index, Some(9));
    }

    #[test]
    fn parse_forecast_rejects_empty_current() {
        assert!(parse_forecast(r#"{"daily":{}}"#).is_err());
        assert!(parse_forecast("not-json").is_err());
    }

    #[test]
    fn parse_geocode_takes_five_named_hits() {
        let json = r#"{
            "results": [
                {"name":"Berlin","latitude":52.52,"longitude":13.41,"country":"Germany","admin1":"Berlin"},
                {"name":"Bergen","latitude":60.39,"longitude":5.32,"country":"Norway"},
                {"name":"SkipMe"},
                {"name":"Paris","latitude":48.85,"longitude":2.35,"country":"France"},
                {"name":"Rome","latitude":41.9,"longitude":12.5,"country":"Italy"},
                {"name":"Madrid","latitude":40.4,"longitude":-3.7,"country":"Spain"},
                {"name":"Lisbon","latitude":38.7,"longitude":-9.1,"country":"Portugal"}
            ]
        }"#;
        let places = parse_geocode(json).unwrap();
        assert_eq!(places.len(), 5);
        assert_eq!(places[0].display_name(), "Berlin, Berlin, Germany");
        assert_eq!(places[1].display_name(), "Bergen, Norway");
        assert!(places.iter().all(|p| p.name != "SkipMe"));
        assert!(places.iter().all(|p| p.name != "Lisbon"));
    }

    #[test]
    fn format_temp_rounds_to_a_degree() {
        assert_eq!(format_temp(18.4), "18°");
        assert_eq!(format_temp(18.6), "19°");
        assert_eq!(format_temp(-1.2), "-1°");
    }

    #[test]
    fn snapshot_freshness_uses_ttl() {
        assert!(snapshot_is_fresh(Instant::now(), CACHE_TTL));
        assert_eq!(CACHE_TTL, Duration::from_secs(30 * 60));
        let stale = Instant::now()
            .checked_sub(CACHE_TTL + Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        if Instant::now().duration_since(stale) > CACHE_TTL {
            assert!(!snapshot_is_fresh(stale, CACHE_TTL));
        }
    }

    #[test]
    fn default_location_is_system_without_coords() {
        let settings = WeatherSettings::default();
        assert!(!settings.enabled);
        assert!(settings.show_on_compact_face);
        assert_eq!(settings.units, WeatherUnits::Celsius);
        assert!(settings.location.coords().is_none());
        assert!(settings.location.is_system());
        assert_eq!(settings.location.display_name(), CURRENT_LOCATION);
    }

    #[test]
    fn stored_manual_city_migrates_to_system_keeping_its_coords() {
        let json = r#"{"enabled":true,"units":"celsius","location":{"mode":"manual","name":"Oslo","lat":59.91,"lon":10.75}}"#;
        let parsed: WeatherSettings = serde_json::from_str(json).unwrap();
        assert!(parsed.location.is_system());
        assert_eq!(parsed.location.coords(), Some((59.91, 10.75)));
        assert_eq!(parsed.location.display_name(), "Oslo");
        let missing: WeatherSettings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(missing.location.is_system());
    }

    #[test]
    fn location_requests_are_throttled_to_the_refresh_window() {
        let now = Instant::now() + LOCATION_REFRESH * 2;
        assert!(location_request_due(None, now), "first run always asks");
        assert!(!location_request_due(Some(now - LOCATION_REFRESH / 2), now));
        assert!(location_request_due(Some(now - LOCATION_REFRESH), now));
    }

    #[test]
    fn small_moves_keep_the_stored_location() {
        let oslo = (59.9139, 10.7522);
        // ~0.5 km north, ~50 km away.
        let near = (oslo.0 + 0.0045, oslo.1);
        let far = (oslo.0 + 0.45, oslo.1);
        assert!(distance_km(oslo, near) < LOCATION_MOVE_KM);
        assert!(distance_km(oslo, far) > 40.0);
        assert!(!location_moved(Some(oslo), near, false));
        assert!(location_moved(Some(oslo), far, false));
        assert!(location_moved(None, near, false), "no stored coords takes any fix");
        assert!(
            location_moved(Some(oslo), near, true),
            "the first fix replaces a migrated manual city"
        );
    }

    #[test]
    fn weather_settings_round_trip() {
        let settings = WeatherSettings {
            enabled: true,
            units: WeatherUnits::Fahrenheit,
            location: WeatherLocationMode::System {
                name: "Oslo".into(),
                lat: 59.91,
                lon: 10.75,
            },
            show_on_compact_face: false,
        };
        let json = serde_json::to_string(&settings).unwrap();
        let parsed: WeatherSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, settings);
        assert_eq!(parsed.location.coords(), Some((59.91, 10.75)));
    }
}
