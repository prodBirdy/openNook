//! Natural-language event / reminder parsing.
//!
//! On macOS, [`NSDataDetector`](https://developer.apple.com/documentation/foundation/nsdatadetector)
//! extracts date/time ranges the same way Mail and Notes do. Everywhere else —
//! and as a fallback — a small English-biased heuristic covers the phrases we
//! ship tests for. Parsing is keystroke-only: call [`parse`] / [`parse_at`]
//! from the UI on each edit. No timers, no polling.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike, Weekday};

/// Whether the line should become a calendar event or a reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Event,
    Reminder,
}

/// A parsed quick-add line, ready for EventKit.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedEntry {
    pub kind: EntryKind,
    pub title: String,
    pub start: f64,
    pub end: f64,
    pub all_day: bool,
    pub location: Option<String>,
}

impl ParsedEntry {
    /// Live preview chip, e.g. `"Lunch with Sam · Tue Aug 25, 12:30–1:30 PM"`.
    pub fn preview_label(&self) -> String {
        let Some(start) = Local.timestamp_opt(self.start as i64, 0).single() else {
            return self.title.clone();
        };
        let Some(end) = Local.timestamp_opt(self.end as i64, 0).single() else {
            return self.title.clone();
        };
        let when = if self.all_day {
            start.format("%a %b %-d").to_string()
        } else {
            format!(
                "{}, {}–{}",
                start.format("%a %b %-d"),
                format_clock(start),
                format_clock(end)
            )
        };
        let loc = self
            .location
            .as_deref()
            .map(|place| format!(" @ {place}"))
            .unwrap_or_default();
        format!("{} · {when}{loc}", self.title)
    }
}

fn format_clock(dt: DateTime<Local>) -> String {
    let hour12 = dt.hour12();
    let h = if hour12.1 == 0 { 12 } else { hour12.1 };
    let suffix = if hour12.0 { "PM" } else { "AM" };
    if dt.minute() == 0 {
        format!("{h} {suffix}")
    } else {
        format!("{h}:{:02} {suffix}", dt.minute())
    }
}

/// Parse `input` relative to now.
pub fn parse(input: &str) -> Option<ParsedEntry> {
    parse_at(input, Local::now())
}

/// Parse `input` relative to `now`. Prefer this in tests.
pub fn parse_at(input: &str, now: DateTime<Local>) -> Option<ParsedEntry> {
    parse_as(input, now, EntryKind::Event)
}

/// Parse `input`, using `default_kind` unless the line starts with a
/// remind / todo / task keyword.
pub fn parse_as(input: &str, now: DateTime<Local>, default_kind: EntryKind) -> Option<ParsedEntry> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    #[cfg(target_os = "macos")]
    if let Some(entry) = macos::parse_with_detector(trimmed, now, default_kind) {
        return Some(entry);
    }

    parse_heuristic(trimmed, now, default_kind)
}

/// True when the line opens with a reminder keyword.
pub fn has_kind_keyword(input: &str) -> bool {
    kind_prefix(input.trim()).is_some()
}

fn parse_heuristic(input: &str, now: DateTime<Local>, default_kind: EntryKind) -> Option<ParsedEntry> {
    let (kind, body) = match kind_prefix(input) {
        Some((_, rest)) => (EntryKind::Reminder, rest),
        None => (default_kind, input),
    };
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let found = find_datetime(body, now);
    let span = found.as_ref().map(|m| m.span);
    let (title, location) = extract_title_location(body, span);
    let title = clean_title(&title);
    if title.is_empty() {
        return None;
    }

    let date = found
        .as_ref()
        .map(|m| m.date)
        .unwrap_or_else(|| now.date_naive());
    let all_day = found.as_ref().is_none_or(|m| m.time.is_none());
    let (start, end) = if all_day {
        let start = local_on(date, NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        (start, start + Duration::days(1))
    } else {
        let match_ = found.as_ref().unwrap();
        let time = match_.time.unwrap();
        let start = local_on(date, time);
        let end = match match_.end_time {
            Some(end_time) => {
                let mut end = local_on(date, end_time);
                if end <= start {
                    end += Duration::days(1);
                }
                end
            }
            None => start + Duration::hours(1),
        };
        (start, end)
    };

    Some(ParsedEntry {
        kind,
        title,
        start: start.timestamp() as f64,
        end: end.timestamp() as f64,
        all_day,
        location,
    })
}

fn kind_prefix(input: &str) -> Option<(usize, &str)> {
    let lower = input.to_ascii_lowercase();
    const PREFIXES: &[&str] = &[
        "remind me to ",
        "remind me ",
        "reminder: ",
        "reminder ",
        "remind ",
        "todo: ",
        "todo ",
        "task: ",
        "task ",
    ];
    for prefix in PREFIXES {
        if lower.starts_with(prefix) {
            return Some((prefix.len(), input[prefix.len()..].trim_start()));
        }
    }
    None
}

#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
}

struct DateMatch {
    date: NaiveDate,
    time: Option<NaiveTime>,
    end_time: Option<NaiveTime>,
    span: Span,
}

fn find_datetime(input: &str, now: DateTime<Local>) -> Option<DateMatch> {
    let lower = input.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut best: Option<DateMatch> = None;
    let mut i = 0;
    while i < bytes.len() {
        if !is_token_start(bytes, i) {
            i += 1;
            continue;
        }
        if let Some(m) = match_datetime_at(&lower, i, now) {
            let end = m.span.end;
            let take = best
                .as_ref()
                .map(|cur| {
                    let cur_len = cur.span.end - cur.span.start;
                    let new_len = m.span.end - m.span.start;
                    new_len > cur_len
                        || (new_len == cur_len && m.time.is_some() && cur.time.is_none())
                })
                .unwrap_or(true);
            if take {
                best = Some(m);
            }
            i = end;
            continue;
        }
        i += 1;
    }
    best
}

fn is_token_start(bytes: &[u8], i: usize) -> bool {
    i == 0 || !bytes[i - 1].is_ascii_alphanumeric()
}

fn match_datetime_at(lower: &str, start: usize, now: DateTime<Local>) -> Option<DateMatch> {
    let mut i = start;
    i = skip_prep(lower, i);
    let after_prep = i;

    if let Some((end, hours, minutes)) = match_relative(lower, i) {
        let when = now + Duration::hours(hours) + Duration::minutes(minutes);
        return Some(DateMatch {
            date: when.date_naive(),
            time: Some(when.time()),
            end_time: None,
            span: Span { start, end },
        });
    }

    let (date, mut i, had_date) = match_date(lower, i, now.date_naive())
        .map(|(date, end)| (date, end, true))
        .unwrap_or((now.date_naive(), i, false));

    i = skip_ws(lower, i);
    i = skip_prep(lower, i);

    let (time, end_time, after_time) = match match_time_range(lower, i) {
        Some((time, end_time, end)) => (Some(time), end_time, end),
        None => (None, None, i),
    };

    if !had_date && time.is_none() {
        return None;
    }

    let mut end = if time.is_some() { after_time } else { i };
    if !had_date && time.is_some() {
        // "tomorrow" may follow the time: "12:30 tomorrow"
        let peek = skip_ws(lower, end);
        if let Some((date2, date_end)) = match_date(lower, peek, now.date_naive()) {
            return Some(DateMatch {
                date: date2,
                time,
                end_time,
                span: Span {
                    start,
                    end: date_end,
                },
            });
        }
    }

    if end <= after_prep && !had_date {
        return None;
    }
    if time.is_none() && !had_date {
        return None;
    }
    if time.is_none() {
        end = i;
    }

    Some(DateMatch {
        date,
        time,
        end_time,
        span: Span { start, end },
    })
}

fn skip_ws(s: &str, i: usize) -> usize {
    let rest = &s[i..];
    let trimmed = rest.trim_start();
    s.len() - trimmed.len()
}

fn skip_prep(s: &str, i: usize) -> usize {
    let i = skip_ws(s, i);
    for prep in ["from ", "until ", "till ", "on ", "at "] {
        if s[i..].starts_with(prep) {
            return skip_ws(s, i + prep.len());
        }
    }
    i
}

fn match_relative(lower: &str, i: usize) -> Option<(usize, i64, i64)> {
    let rest = &lower[i..];
    if let Some(rest) = rest.strip_prefix("in ") {
        let rest = rest.trim_start();
        let offset = lower.len() - rest.len();
        if rest.starts_with("an hour") || rest.starts_with("a hour") {
            let end = consume_word_from(lower, offset, if rest.starts_with("an ") { 2 } else { 2 });
            return Some((end, 1, 0));
        }
        let (n, after_n) = take_u32(rest)?;
        let after = rest[after_n..].trim_start();
        let abs = lower.len() - after.len();
        if after.starts_with("hours") || after.starts_with("hour") {
            let word = if after.starts_with("hours") { "hours" } else { "hour" };
            return Some((abs + word.len(), n as i64, 0));
        }
        if after.starts_with("minutes") || after.starts_with("minute") || after.starts_with("mins")
        {
            let word = if after.starts_with("minutes") {
                "minutes"
            } else if after.starts_with("minute") {
                "minute"
            } else {
                "mins"
            };
            return Some((abs + word.len(), 0, n as i64));
        }
    }
    None
}

fn consume_word_from(s: &str, start: usize, words: usize) -> usize {
    let mut i = start;
    for _ in 0..words {
        i = skip_ws(s, i);
        while i < s.len() && s.as_bytes()[i].is_ascii_alphabetic() {
            i += 1;
        }
    }
    i
}

fn match_date(lower: &str, i: usize, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    let rest = &lower[i..];
    if rest.is_empty() {
        return None;
    }

    if let Some(end) = starts_word(rest, "today") {
        return Some((today, i + end));
    }
    if let Some(end) = starts_word(rest, "heute") {
        return Some((today, i + end));
    }
    if let Some(end) = starts_word(rest, "tomorrow") {
        return Some((today + Duration::days(1), i + end));
    }
    if let Some(end) = starts_word(rest, "morgen") {
        return Some((today + Duration::days(1), i + end));
    }
    if let Some(end) = starts_word(rest, "yesterday") {
        return Some((today - Duration::days(1), i + end));
    }
    if let Some(end) = starts_word(rest, "tonight") {
        return Some((today, i + end));
    }
    if rest.starts_with("day after tomorrow") {
        return Some((today + Duration::days(2), i + "day after tomorrow".len()));
    }
    if let Some(end) = starts_word(rest, "übermorgen") {
        return Some((today + Duration::days(2), i + end));
    }

    for (label, hour) in [
        ("this morning", 9),
        ("this afternoon", 14),
        ("this evening", 19),
    ] {
        if rest.starts_with(label) {
            let _ = hour;
            return Some((today, i + label.len()));
        }
    }

    let mut cur = i;
    let mut next = false;
    if rest.starts_with("next ") {
        next = true;
        cur = skip_ws(lower, i + 5);
    } else if rest.starts_with("this ") {
        cur = skip_ws(lower, i + 5);
    }

    if let Some((wd, end)) = match_weekday(&lower[cur..]) {
        let date = next_weekday(today, wd, next);
        return Some((date, cur + end));
    }

    if let Some((date, end)) = match_month_day(&lower[i..], today) {
        return Some((date, i + end));
    }

    None
}

fn starts_word(rest: &str, word: &str) -> Option<usize> {
    if rest.starts_with(word) {
        let after = word.len();
        if after == rest.len() || !rest.as_bytes()[after].is_ascii_alphanumeric() {
            return Some(after);
        }
    }
    None
}

fn match_weekday(rest: &str) -> Option<(Weekday, usize)> {
    const DAYS: &[(&str, Weekday)] = &[
        ("monday", Weekday::Mon),
        ("mon", Weekday::Mon),
        ("tuesday", Weekday::Tue),
        ("tue", Weekday::Tue),
        ("tues", Weekday::Tue),
        ("wednesday", Weekday::Wed),
        ("wed", Weekday::Wed),
        ("thursday", Weekday::Thu),
        ("thu", Weekday::Thu),
        ("thur", Weekday::Thu),
        ("thurs", Weekday::Thu),
        ("friday", Weekday::Fri),
        ("fri", Weekday::Fri),
        ("saturday", Weekday::Sat),
        ("sat", Weekday::Sat),
        ("sunday", Weekday::Sun),
        ("sun", Weekday::Sun),
    ];
    let mut best: Option<(Weekday, usize)> = None;
    for (name, day) in DAYS {
        if let Some(end) = starts_word(rest, name) {
            if best.as_ref().map(|(_, n)| end > *n).unwrap_or(true) {
                best = Some((*day, end));
            }
        }
    }
    best
}

fn next_weekday(today: NaiveDate, weekday: Weekday, force_next: bool) -> NaiveDate {
    let mut date = today;
    if force_next && date.weekday() == weekday {
        date += Duration::days(7);
        return date;
    }
    for _ in 0..7 {
        if date.weekday() == weekday {
            return date;
        }
        date += Duration::days(1);
    }
    today
}

fn match_month_day(rest: &str, today: NaiveDate) -> Option<(NaiveDate, usize)> {
    if let Some((month, after_month)) = match_month_name(rest) {
        let after = rest[after_month..].trim_start();
        let (day, after_day) = take_u32(after)?;
        if !(1..=31).contains(&day) {
            return None;
        }
        let after_num = after[after_day..].trim_start();
        let (year, year_len) = take_year(after_num).unwrap_or((today.year(), 0));
        let date = bump_past(NaiveDate::from_ymd_opt(year, month, day)?, today);
        let end = rest.len() - after_num.len() + year_len;
        return Some((date, end));
    }

    let (day, after_day) = take_u32(rest)?;
    if !(1..=31).contains(&day) {
        return None;
    }
    let after = rest[after_day..].trim_start();
    let (month, after_month) = match_month_name(after)?;
    let after_m = after[after_month..].trim_start();
    let (year, year_len) = take_year(after_m).unwrap_or((today.year(), 0));
    let date = bump_past(NaiveDate::from_ymd_opt(year, month, day)?, today);
    let end = rest.len() - after_m.len() + year_len;
    Some((date, end))
}

fn match_month_name(rest: &str) -> Option<(u32, usize)> {
    const MONTHS: &[(&str, u32)] = &[
        ("january", 1),
        ("jan", 1),
        ("february", 2),
        ("feb", 2),
        ("march", 3),
        ("mar", 3),
        ("april", 4),
        ("apr", 4),
        ("may", 5),
        ("june", 6),
        ("jun", 6),
        ("july", 7),
        ("jul", 7),
        ("august", 8),
        ("aug", 8),
        ("september", 9),
        ("sept", 9),
        ("sep", 9),
        ("october", 10),
        ("oct", 10),
        ("november", 11),
        ("nov", 11),
        ("december", 12),
        ("dec", 12),
    ];
    let mut best: Option<(u32, usize)> = None;
    for (name, month) in MONTHS {
        if let Some(end) = starts_word(rest, name) {
            if best.as_ref().map(|(_, n)| end > *n).unwrap_or(true) {
                best = Some((*month, end));
            }
        }
    }
    best
}

fn take_year(rest: &str) -> Option<(i32, usize)> {
    let (n, len) = take_u32(rest)?;
    if len == 4 && (1970..=2100).contains(&n) {
        Some((n as i32, len))
    } else {
        None
    }
}

fn bump_past(date: NaiveDate, today: NaiveDate) -> NaiveDate {
    if date < today {
        date.with_year(date.year() + 1).unwrap_or(date)
    } else {
        date
    }
}

fn take_u32(s: &str) -> Option<(u32, usize)> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let n = digits.parse().ok()?;
    Some((n, digits.len()))
}

fn match_time_range(lower: &str, i: usize) -> Option<(NaiveTime, Option<NaiveTime>, usize)> {
    let (start, after_start) = match_time(lower, i)?;
    let mut j = skip_ws(lower, after_start);
    if j < lower.len() {
        let rest = &lower[j..];
        if rest.starts_with('-') || rest.starts_with('–') || rest.starts_with('—') {
            j = skip_ws(lower, j + rest.chars().next().unwrap().len_utf8());
            if let Some((end, after_end)) = match_time(lower, j) {
                return Some((start, Some(end), after_end));
            }
        } else if rest.starts_with("to ") {
            j = skip_ws(lower, j + 3);
            if let Some((end, after_end)) = match_time(lower, j) {
                return Some((start, Some(end), after_end));
            }
        }
    }
    Some((start, None, after_start))
}

fn match_time(lower: &str, i: usize) -> Option<(NaiveTime, usize)> {
    let rest = &lower[i..];
    if let Some(end) = starts_word(rest, "noon") {
        return Some((NaiveTime::from_hms_opt(12, 0, 0).unwrap(), i + end));
    }
    if let Some(end) = starts_word(rest, "midnight") {
        return Some((NaiveTime::from_hms_opt(0, 0, 0).unwrap(), i + end));
    }

    let (hour, after_h) = take_u32(rest)?;
    if hour > 23 {
        return None;
    }
    let mut cur = after_h;
    let mut minute = 0u32;
    if rest[cur..].starts_with(':') {
        cur += 1;
        let (m, after_m) = take_u32(&rest[cur..])?;
        if m > 59 {
            return None;
        }
        minute = m;
        cur += after_m;
    } else if cur < rest.len() && rest.as_bytes()[cur].is_ascii_digit() {
        return None;
    }

    let after_num = cur;
    let mut peek = rest[cur..].trim_start();
    let mut meridiem = None;
    if let Some(stripped) = peek.strip_prefix("a.m.") {
        meridiem = Some(false);
        peek = stripped;
    } else if let Some(stripped) = peek.strip_prefix("p.m.") {
        meridiem = Some(true);
        peek = stripped;
    } else if let Some(stripped) = peek.strip_prefix("am") {
        meridiem = Some(false);
        peek = stripped;
    } else if let Some(stripped) = peek.strip_prefix("pm") {
        meridiem = Some(true);
        peek = stripped;
    } else if let Some(stripped) = peek.strip_prefix('a') {
        if stripped.is_empty() || !stripped.as_bytes()[0].is_ascii_alphanumeric() {
            meridiem = Some(false);
            peek = stripped;
        }
    } else if let Some(stripped) = peek.strip_prefix('p') {
        if stripped.is_empty() || !stripped.as_bytes()[0].is_ascii_alphanumeric() {
            meridiem = Some(true);
            peek = stripped;
        }
    }

    let hour = apply_meridiem(hour, minute, meridiem, after_num > after_h)?;
    let time = NaiveTime::from_hms_opt(hour, minute, 0)?;
    let consumed = rest.len() - peek.len();
    Some((time, i + consumed))
}

fn apply_meridiem(hour: u32, minute: u32, meridiem: Option<bool>, had_colon: bool) -> Option<u32> {
    match meridiem {
        Some(false) => {
            if hour == 12 {
                Some(0)
            } else if (1..=11).contains(&hour) {
                Some(hour)
            } else {
                None
            }
        }
        Some(true) => {
            if hour == 12 {
                Some(12)
            } else if (1..=11).contains(&hour) {
                Some(hour + 12)
            } else {
                None
            }
        }
        None => {
            if hour > 23 || minute > 59 {
                return None;
            }
            if had_colon {
                // 12:30 stays 12:30; 15:00 stays 15:00.
                Some(hour)
            } else if (1..=7).contains(&hour) {
                // "meeting at 3" → 15:00
                Some(hour + 12)
            } else {
                Some(hour)
            }
        }
    }
}

fn extract_title_location(input: &str, span: Option<Span>) -> (String, Option<String>) {
    let remainder = if let Some(span) = span {
        let start = span.start.min(input.len());
        let end = span.end.min(input.len());
        let mut out = String::new();
        out.push_str(input[..start].trim());
        if !out.is_empty() && end < input.len() {
            out.push(' ');
        }
        out.push_str(input[end..].trim());
        out
    } else {
        input.to_string()
    };
    split_location(&remainder)
}

fn split_location(text: &str) -> (String, Option<String>) {
    let lower = text.to_ascii_lowercase();
    if let Some(idx) = find_at_place(&lower) {
        let title = text[..idx].trim();
        let place = text[idx + 3..].trim();
        if !place.is_empty() && !looks_like_time_phrase(place) {
            return (title.to_string(), Some(collapse_ws(place)));
        }
    }
    (collapse_ws(text.trim()), None)
}

fn find_at_place(lower: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(rel) = lower[search..].find(" at ") {
        let idx = search + rel;
        let after = lower[idx + 4..].trim_start();
        if !looks_like_time_phrase(after) {
            return Some(idx);
        }
        search = idx + 4;
    }
    None
}

fn looks_like_time_phrase(s: &str) -> bool {
    let s = s.trim();
    match_time(&s.to_ascii_lowercase(), 0).is_some()
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_title(title: &str) -> String {
    let mut t = collapse_ws(title);
    for prep in [" on", " at", " from", " until", " till"] {
        if let Some(stripped) = t
            .to_ascii_lowercase()
            .strip_suffix(prep)
            .map(|_| t.len() - prep.len())
        {
            t.truncate(stripped);
            t = t.trim().to_string();
        }
    }
    t
}

fn local_on(date: NaiveDate, time: NaiveTime) -> DateTime<Local> {
    date.and_time(time)
        .and_local_timezone(Local)
        .single()
        .or_else(|| date.and_time(time).and_local_timezone(Local).earliest())
        .unwrap_or_else(|| Local.from_utc_datetime(&date.and_time(time)))
}

/// Strip a detector (or heuristic) date span plus dangling prepositions.
pub fn strip_date_span(input: &str, utf8_range: std::ops::Range<usize>) -> String {
    let start = utf8_range.start.min(input.len());
    let end = utf8_range.end.min(input.len());
    let (title, _) = extract_title_location(
        input,
        Some(Span { start, end }),
    );
    clean_title(&title)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2_foundation::{
        NSDataDetector, NSMatchingOptions, NSRange, NSString, NSTextCheckingType,
        NSTextCheckingTypes,
    };
    use std::sync::OnceLock;

    fn detector() -> Option<&'static Retained<NSDataDetector>> {
        static DETECTOR: OnceLock<Option<Retained<NSDataDetector>>> = OnceLock::new();
        DETECTOR
            .get_or_init(|| {
                let types: NSTextCheckingTypes =
                    NSTextCheckingType::Date.0 | NSTextCheckingType::Address.0;
                NSDataDetector::dataDetectorWithTypes_error(types).ok()
            })
            .as_ref()
    }

    fn is_date(kind: NSTextCheckingType) -> bool {
        kind.0 & NSTextCheckingType::Date.0 != 0
    }

    fn is_address(kind: NSTextCheckingType) -> bool {
        kind.0 & NSTextCheckingType::Address.0 != 0
    }

    pub(super) fn parse_with_detector(
        input: &str,
        now: DateTime<Local>,
        default_kind: EntryKind,
    ) -> Option<ParsedEntry> {
        let detector = detector()?;
        let ns = NSString::from_str(input);
        let full = NSRange {
            location: 0,
            length: ns.length(),
        };
        let matches = unsafe {
            detector.matchesInString_options_range(&ns, NSMatchingOptions::empty(), full)
        };

        let mut date_span: Option<(f64, f64, std::ops::Range<usize>, bool)> = None;
        let mut location: Option<String> = None;

        for result in matches.iter() {
            let kind = unsafe { result.resultType() };
            if is_date(kind) {
                if date_span.is_some() {
                    continue;
                }
                let Some(date) = (unsafe { result.date() }) else {
                    continue;
                };
                let start = date.timeIntervalSince1970();
                let duration = unsafe { result.duration() };
                let range = unsafe { result.range() };
                let utf8 = utf16_range_to_utf8(input, range.location, range.length);
                let matched = input.get(utf8.clone()).unwrap_or("");
                let all_day = !has_time_token(matched);
                let end = if duration > 0.0 {
                    start + duration
                } else if all_day {
                    start + 24.0 * 60.0 * 60.0
                } else {
                    start + 60.0 * 60.0
                };
                date_span = Some((start, end, utf8, all_day));
            } else if is_address(kind) {
                let range = unsafe { result.range() };
                let utf8 = utf16_range_to_utf8(input, range.location, range.length);
                if let Some(place) = input.get(utf8) {
                    let place = place.trim();
                    if !place.is_empty() {
                        location = Some(place.to_string());
                    }
                }
            }
        }

        // Detector found nothing useful — let the heuristic path run.
        if date_span.is_none() && location.is_none() {
            return None;
        }

        let (kind, body) = match kind_prefix(input) {
            Some((_, rest)) => (EntryKind::Reminder, rest),
            None => (default_kind, input),
        };

        let (start, end, span, all_day) = date_span.unwrap_or_else(|| {
            let start = now.timestamp() as f64;
            (
                start,
                start + 24.0 * 60.0 * 60.0,
                0..0,
                true,
            )
        });

        let (mut title, heuristic_loc) = extract_title_location(
            body,
            if span.end > span.start && body.len() == input.len() {
                Some(Span {
                    start: span.start,
                    end: span.end,
                })
            } else if span.end > span.start {
                // Body is shorter than input (kind prefix stripped). Shift the span.
                let shift = input.len().saturating_sub(body.len());
                let start = span.start.saturating_sub(shift);
                let end = span.end.saturating_sub(shift);
                Some(Span { start, end })
            } else {
                None
            },
        );
        title = clean_title(&title);
        if title.is_empty() {
            title = clean_title(body);
        }
        if title.is_empty() {
            return None;
        }
        if location.is_none() {
            location = heuristic_loc;
        }

        Some(ParsedEntry {
            kind,
            title,
            start,
            end,
            all_day,
            location,
        })
    }

    fn utf16_range_to_utf8(text: &str, location: usize, length: usize) -> std::ops::Range<usize> {
        let start = utf16_to_utf8(text, location);
        let end = utf16_to_utf8(text, location.saturating_add(length));
        start..end
    }

    fn utf16_to_utf8(text: &str, utf16: usize) -> usize {
        let mut seen = 0;
        for (i, c) in text.char_indices() {
            if seen >= utf16 {
                return i;
            }
            seen += c.len_utf16();
        }
        text.len()
    }

    fn has_time_token(matched: &str) -> bool {
        let lower = matched.to_ascii_lowercase();
        if lower.contains("am")
            || lower.contains("pm")
            || lower.contains(':')
            || lower.contains("noon")
            || lower.contains("midnight")
        {
            return true;
        }
        // Digit run next to am/pm already covered; a lone hour like "3" is a time.
        lower.split(|c: char| !c.is_ascii_digit()).any(|d| {
            !d.is_empty()
                && d.parse::<u32>()
                    .ok()
                    .is_some_and(|n| (1..=23).contains(&n))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 8, 28, 10, 0, 0)
            .single()
            .expect("fixed local 2026-08-28 10:00")
    }

    fn parsed(input: &str) -> ParsedEntry {
        parse_at(input, now()).expect(input)
    }

    fn local_parts(ts: f64) -> (NaiveDate, u32, u32) {
        let dt = Local.timestamp_opt(ts as i64, 0).single().unwrap();
        (dt.date_naive(), dt.hour(), dt.minute())
    }

    #[test]
    fn empty_input_is_none() {
        assert!(parse_at("", now()).is_none());
        assert!(parse_at("   ", now()).is_none());
    }

    #[test]
    fn lunch_tomorrow_half_past() {
        let e = parsed("lunch with Sam tomorrow 12:30");
        assert_eq!(e.kind, EntryKind::Event);
        assert_eq!(e.title, "lunch with Sam");
        assert!(!e.all_day);
        assert_eq!(e.location, None);
        let (date, h, m) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 29).unwrap());
        assert_eq!((h, m), (12, 30));
        let (_, eh, em) = local_parts(e.end);
        assert_eq!((eh, em), (13, 30));
    }

    #[test]
    fn remind_me_keyword_routes_to_reminder() {
        let e = parsed("remind me to call mom at 5pm");
        assert_eq!(e.kind, EntryKind::Reminder);
        assert_eq!(e.title, "call mom");
        assert!(!e.all_day);
        let (date, h, m) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 28).unwrap());
        assert_eq!((h, m), (17, 0));
    }

    #[test]
    fn todo_without_time_is_all_day() {
        let e = parsed("todo buy milk");
        assert_eq!(e.kind, EntryKind::Reminder);
        assert_eq!(e.title, "buy milk");
        assert!(e.all_day);
    }

    #[test]
    fn next_tuesday_afternoon() {
        let e = parsed("next tue 3pm standup");
        assert_eq!(e.title, "standup");
        assert!(!e.all_day);
        let (date, h, m) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap());
        assert_eq!((h, m), (15, 0));
    }

    #[test]
    fn month_day_time_range() {
        let e = parsed("aug 30 9-10am dentist");
        assert_eq!(e.title, "dentist");
        assert!(!e.all_day);
        let (date, h, m) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 30).unwrap());
        assert_eq!((h, m), (9, 0));
        let (_, eh, em) = local_parts(e.end);
        assert_eq!((eh, em), (10, 0));
    }

    #[test]
    fn location_at_place_vs_at_time() {
        let e = parsed("lunch with Sam at Blue Bottle tomorrow");
        assert_eq!(e.title, "lunch with Sam");
        assert_eq!(e.location.as_deref(), Some("Blue Bottle"));
        assert!(e.all_day);
        let (date, _, _) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 29).unwrap());

        let timed = parsed("lunch with Sam at 12:30 tomorrow");
        assert_eq!(timed.title, "lunch with Sam");
        assert_eq!(timed.location, None);
        assert!(!timed.all_day);
    }

    #[test]
    fn default_duration_is_sixty_minutes() {
        let e = parsed("standup at 3pm");
        assert_eq!((e.end as i64) - (e.start as i64), 3600);
    }

    #[test]
    fn time_range_three_to_five() {
        let e = parsed("design review 3-5pm");
        let (_, h, _) = local_parts(e.start);
        let (_, eh, _) = local_parts(e.end);
        assert_eq!((h, eh), (15, 17));
        assert_eq!(e.title, "design review");
    }

    #[test]
    fn parse_as_keeps_card_default_without_keyword() {
        let e = parse_as("buy milk tomorrow", now(), EntryKind::Reminder).unwrap();
        assert_eq!(e.kind, EntryKind::Reminder);
        assert_eq!(e.title, "buy milk");
        let event = parse_as("buy milk tomorrow", now(), EntryKind::Event).unwrap();
        assert_eq!(event.kind, EntryKind::Event);
    }

    #[test]
    fn german_morgen() {
        let e = parsed("kaffee morgen 12:30");
        assert_eq!(e.title, "kaffee");
        let (date, h, m) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 29).unwrap());
        assert_eq!((h, m), (12, 30));
    }

    #[test]
    fn relative_in_two_hours() {
        let e = parsed("stretch in 2 hours");
        assert_eq!(e.title, "stretch");
        let (_, h, m) = local_parts(e.start);
        assert_eq!((h, m), (12, 0));
    }

    #[test]
    fn preview_label_includes_title_and_time() {
        let e = parsed("lunch with Sam tomorrow 12:30");
        let label = e.preview_label();
        assert!(label.starts_with("lunch with Sam · "), "{label}");
        assert!(label.contains("12:30"), "{label}");
        assert!(label.contains("1:30"), "{label}");
    }

    #[test]
    fn strip_date_span_drops_prepositions() {
        assert_eq!(
            strip_date_span("standup on tomorrow", 11..19),
            "standup"
        );
    }

    #[test]
    fn has_kind_keyword_detects_prefixes() {
        assert!(has_kind_keyword("remind me to breathe"));
        assert!(has_kind_keyword("TODO buy oats"));
        assert!(!has_kind_keyword("lunch with Sam"));
    }

    #[test]
    fn friday_without_next_is_today_when_today_is_friday() {
        let e = parsed("team offsite friday");
        assert_eq!(e.title, "team offsite");
        assert!(e.all_day);
        let (date, _, _) = local_parts(e.start);
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 8, 28).unwrap());
    }
}
