//! A hand-rolled port of the format families PHP's own relative/absolute
//! date-string grammar (`ext/date/lib/parse_date.re`) accepts most often
//! in real code, used to validate a literal string passed to
//! `new DateTime()`/`new DateTimeImmutable()` (`checker::date_time`) and
//! to narrow `strtotime()`'s own return type (`checker::expr::builtin_
//! calls`'s `"strtotime"` arm, which now calls `validate_relative_delta`
//! here instead of keeping its own separate copy of this one grammar
//! fragment). `parse_date.re` itself isn't vendored anywhere in this
//! repo - every accepted-shape decision below was cross-checked directly
//! against a real local `php -r 'var_dump(strtotime(...))'`
//! (PHP 8.5.4), not recalled knowledge; there is no vendored ground
//! truth to point a reader at instead.
//!
//! Real PHP's own grammar is far larger than what's covered here
//! (natural-language month names, ordinal weekdays like `"2nd tuesday of
//! next month"`, ambiguous slash-separated dates, and more) - this is
//! deliberately scoped to the format families real-world code actually
//! uses most, per this project's own "verify, don't guess" discipline:
//! `validate_date_time_string` returns `None` ("not a shape this
//! recognizes at all") for anything outside that scope, rather than
//! guessing it must be malformed. Only `Some(false)` - a recognized
//! shape whose own inner fields are provably out of range - is ever
//! treated as a real "this throws at runtime" finding; an unrecognized
//! shape is exactly as likely to be one of PHP's many uncovered formats
//! as it is to be a typo, so staying silent is the only sound answer.

/// Top-level entry point - `Some(true)`/`Some(false)` once a recognized
/// family accepts or rejects `s`, `None` once every family has declined
/// to recognize its own shape at all.
pub(crate) fn validate_date_time_string(s: &str) -> Option<bool> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    validate_timestamp(s)
        .or_else(|| validate_iso_date_time(s))
        .or_else(|| validate_relative_delta(s))
        .or_else(|| validate_next_last(s))
        .or_else(|| validate_first_last_day_of(s))
        .or_else(|| validate_keyword(s))
}

/// `@<seconds>[.<fraction>]` - always accepted once the shape matches;
/// PHP's own Unix-timestamp form has no range restriction of its own
/// (negative, zero, and arbitrarily large timestamps are all valid).
fn validate_timestamp(s: &str) -> Option<bool> {
    let rest = s.strip_prefix('@')?;
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    let (int_part, frac_part) = rest.split_once('.').unwrap_or((rest, ""));
    if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !frac_part.is_empty() && !frac_part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(true)
}

/// `YYYY[-MM[-DD]]` optionally followed by `[T ]HH[:MI[:SS[.ffffff]]]`
/// optionally followed by `Z`/`+HH:MM`/`-HH:MM`/`+HHMM`/`-HHMM`. Real PHP
/// is far more lenient than a calendar-accurate check (`2024-02-30`,
/// `2024-00-15`, `2024-01-00` all succeed, silently rolling over) - only
/// each field's own fixed-width numeric *range* is validated (month
/// 0-12, day 0-31, hour 0-24, minute 0-59, second 0-60 for leap-second
/// tolerance), matching what was actually observed, not a stricter
/// calendar-correct guess.
fn validate_iso_date_time(s: &str) -> Option<bool> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let digits = |b: &[u8], i: usize| b[i..].iter().take_while(|c| c.is_ascii_digit()).count();

    let year_len = digits(bytes, i);
    if !(1..=4).contains(&year_len) {
        return None;
    }
    i += year_len;

    let consume_field = |i: &mut usize, sep: u8, max: u32| -> Option<bool> {
        if *i >= bytes.len() || bytes[*i] != sep {
            return None;
        }
        let start = *i + 1;
        let len = digits(bytes, start);
        if !(1..=2).contains(&len) {
            return None;
        }
        let value: u32 = s[start..start + len].parse().ok()?;
        *i = start + len;
        Some(value <= max)
    };

    let month_ok = match consume_field(&mut i, b'-', 12) {
        Some(ok) => ok,
        // Bare `YYYY` is only genuinely recognized once it's the whole
        // string - trailing, unseparated garbage (`"2024x"`) is an
        // unrecognized shape, not a confidently-valid bare year.
        None => return if i == bytes.len() { Some(true) } else { None },
    };
    if !month_ok {
        return Some(false);
    }
    let day_ok = match consume_field(&mut i, b'-', 31) {
        Some(ok) => ok,
        None => return if i == bytes.len() { Some(true) } else { None },
    };
    if !day_ok {
        return Some(false);
    }

    if i >= bytes.len() {
        return Some(true);
    }
    if bytes[i] != b'T' && bytes[i] != b' ' {
        return None;
    }
    i += 1;

    let hour_len = digits(bytes, i);
    if !(1..=2).contains(&hour_len) {
        return None;
    }
    let Ok(hour) = s[i..i + hour_len].parse::<u32>() else {
        return None;
    };
    if hour > 24 {
        return Some(false);
    }
    i += hour_len;

    let minute = match consume_field(&mut i, b':', 59) {
        Some(ok) => {
            if !ok {
                return Some(false);
            }
            true
        }
        None => false,
    };
    if minute {
        match consume_field(&mut i, b':', 60) {
            Some(true) => {
                // Optional fractional seconds (`.ffffff`).
                if i < bytes.len() && bytes[i] == b'.' {
                    let frac_start = i + 1;
                    let frac_len = digits(bytes, frac_start);
                    if frac_len == 0 {
                        return None;
                    }
                    i = frac_start + frac_len;
                }
            }
            Some(false) => return Some(false),
            None => {}
        }
    }

    if i >= bytes.len() {
        return Some(true);
    }
    // Timezone suffix: `Z`, or `[+-]HH:MM`/`[+-]HHMM`.
    if bytes[i] == b'Z' && i + 1 == bytes.len() {
        return Some(true);
    }
    if bytes[i] == b'+' || bytes[i] == b'-' {
        let tz = &s[i + 1..];
        let tz_no_colon = tz.replace(':', "");
        if tz_no_colon.len() == 4 && tz_no_colon.bytes().all(|b| b.is_ascii_digit()) {
            return Some(true);
        }
        return None;
    }
    None
}

/// Every unit real PHP's relative-delta grammar accepts, singular form
/// only (the caller strips a trailing `s`).
const RELATIVE_UNITS: &[&str] = &[
    "second",
    "sec",
    "minute",
    "min",
    "hour",
    "day",
    "fortnight",
    "week",
    "month",
    "year",
    "weekday",
];

/// `[+-]? N unit(s) [ago]` - the single-delta family already used to
/// narrow `strtotime()`'s own return type, now the one shared
/// implementation (was `checker::expr::builtin_calls::is_relative_time_
/// offset_literal`, whose own unit table was missing `fortnight`/
/// `weekday` - both real, accepted units this project's own corpus
/// hasn't hit yet but real PHP does support, e.g. `"2 fortnights"`/
/// `"+2 weekdays"`).
pub(crate) fn validate_relative_delta(s: &str) -> Option<bool> {
    let s = s.trim();
    let bytes = s.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start || i >= bytes.len() || bytes[i] != b' ' {
        return None;
    }
    while i < bytes.len() && bytes[i] == b' ' {
        i += 1;
    }
    let unit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if unit_start == i {
        return None;
    }
    let unit_lower = s[unit_start..i].to_ascii_lowercase();
    let unit = unit_lower.strip_suffix('s').unwrap_or(&unit_lower);
    if !RELATIVE_UNITS.contains(&unit) {
        return None;
    }
    let rest = s[i..].trim();
    if rest.is_empty() || rest.eq_ignore_ascii_case("ago") {
        Some(true)
    } else {
        None
    }
}

const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "weekday",
    "weekdays",
];

/// `next <weekday|unit>` / `last <weekday|unit>` (`"next monday"`,
/// `"last month"`, `"next week"`, `"last fortnight"`).
fn validate_next_last(s: &str) -> Option<bool> {
    let lower = s.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("next ")
        .or_else(|| lower.strip_prefix("last "))?;
    let rest = rest.trim();
    if WEEKDAYS.contains(&rest) {
        return Some(true);
    }
    let singular = rest.strip_suffix('s').unwrap_or(rest);
    if RELATIVE_UNITS.contains(&singular) {
        return Some(true);
    }
    None
}

/// `first day of ...` / `last day of ...` - real PHP recurses into a
/// full nested date/relative expression for whatever follows `of`
/// (including nothing at all - `"first day of"` alone is valid, meaning
/// "of this month"). Never invalidated once the prefix itself matches:
/// the trailing clause's own grammar isn't independently re-verified
/// here, matching this module's own "don't guess a malformed verdict"
/// discipline for a shape this doesn't fully model.
fn validate_first_last_day_of(s: &str) -> Option<bool> {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("first day of") || lower.starts_with("last day of") {
        Some(true)
    } else {
        None
    }
}

const KEYWORDS: &[&str] = &["now", "today", "tomorrow", "yesterday", "midnight", "noon"];

fn validate_keyword(s: &str) -> Option<bool> {
    if KEYWORDS.contains(&s.to_ascii_lowercase().as_str()) {
        Some(true)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_always_valid() {
        assert_eq!(validate_date_time_string("@1700000000"), Some(true));
        assert_eq!(validate_date_time_string("@-100"), Some(true));
        assert_eq!(validate_date_time_string("@1700000000.5"), Some(true));
        assert_eq!(validate_date_time_string("@abc"), None);
    }

    #[test]
    fn iso_dates_validate_field_ranges_not_calendar_correctness() {
        assert_eq!(validate_date_time_string("2024-01-15"), Some(true));
        assert_eq!(validate_date_time_string("2024-02-30"), Some(true));
        assert_eq!(validate_date_time_string("2024-00-15"), Some(true));
        assert_eq!(validate_date_time_string("2024-01-00"), Some(true));
        assert_eq!(validate_date_time_string("2024-13-01"), Some(false));
        assert_eq!(validate_date_time_string("2024-01-32"), Some(false));
        assert_eq!(validate_date_time_string("2024"), Some(true));
        assert_eq!(validate_date_time_string("2024-01"), Some(true));
    }

    #[test]
    fn iso_date_times_validate_time_field_ranges() {
        assert_eq!(validate_date_time_string("2024-01-15T10:30:00"), Some(true));
        assert_eq!(validate_date_time_string("2024-01-15 10:30:00"), Some(true));
        assert_eq!(validate_date_time_string("2024-01-15T24:00:00"), Some(true));
        assert_eq!(validate_date_time_string("2024-01-15T23:59:60"), Some(true));
        assert_eq!(
            validate_date_time_string("2024-01-15T23:60:00"),
            Some(false)
        );
        assert_eq!(
            validate_date_time_string("2024-01-15T25:00:00"),
            Some(false)
        );
    }

    #[test]
    fn iso_date_times_accept_a_timezone_suffix() {
        assert_eq!(
            validate_date_time_string("2024-01-15T10:30:00Z"),
            Some(true)
        );
        assert_eq!(
            validate_date_time_string("2024-01-15T10:30:00+02:00"),
            Some(true)
        );
        assert_eq!(
            validate_date_time_string("2024-01-15T10:30:00+0200"),
            Some(true)
        );
    }

    #[test]
    fn relative_deltas_cover_the_full_unit_table() {
        assert_eq!(validate_date_time_string("+1 day"), Some(true));
        assert_eq!(validate_date_time_string("-30 days"), Some(true));
        assert_eq!(validate_date_time_string("1 fortnight"), Some(true));
        assert_eq!(validate_date_time_string("3 fortnights"), Some(true));
        assert_eq!(validate_date_time_string("+2 weekdays"), Some(true));
        assert_eq!(validate_date_time_string("2 weeks ago"), Some(true));
    }

    #[test]
    fn next_last_covers_weekdays_and_units() {
        assert_eq!(validate_date_time_string("next monday"), Some(true));
        assert_eq!(validate_date_time_string("last friday"), Some(true));
        assert_eq!(validate_date_time_string("next week"), Some(true));
        assert_eq!(validate_date_time_string("last month"), Some(true));
        assert_eq!(validate_date_time_string("next weekday"), Some(true));
        assert_eq!(validate_date_time_string("next 2 days"), None);
    }

    #[test]
    fn first_last_day_of_is_never_invalidated() {
        assert_eq!(validate_date_time_string("first day of"), Some(true));
        assert_eq!(
            validate_date_time_string("first day of january"),
            Some(true)
        );
        assert_eq!(
            validate_date_time_string("first day of next month"),
            Some(true)
        );
        assert_eq!(
            validate_date_time_string("last day of this month"),
            Some(true)
        );
    }

    #[test]
    fn keywords_are_recognized_case_insensitively() {
        assert_eq!(validate_date_time_string("now"), Some(true));
        assert_eq!(validate_date_time_string("Today"), Some(true));
        assert_eq!(validate_date_time_string("TOMORROW"), Some(true));
        assert_eq!(validate_date_time_string("midnight"), Some(true));
        assert_eq!(validate_date_time_string("noon"), Some(true));
    }

    #[test]
    fn an_unrecognized_shape_stays_unknown_not_malformed() {
        assert_eq!(validate_date_time_string("January 15, 2024"), None);
        assert_eq!(validate_date_time_string("gibberish"), None);
        assert_eq!(validate_date_time_string(""), None);
    }
}
