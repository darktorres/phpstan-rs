//! `DateInterval`'s ISO-8601 duration-string constructor validation
//! (PHPStan's `DateIntervalInstantiationRule`, which constructs the
//! object at analysis time). With no PHP runtime to do that, this grammar
//! mirrors PHP's two accepted spec formats instead; every malformed spec
//! gets PHP's one exception message, `Unknown or bad format (<spec>)`.

use crate::checker::{Checker, Locals};
use syntax::ast::{Arg, NodeArenas};
use syntax::span::Span;

impl<'a> Checker<'a> {
    /// `new DateInterval($spec)` - real PHP throws
    /// `DateMalformedIntervalStringException` (an `Exception` subclass on
    /// PHP < 8.3) when `$spec` matches neither of its two accepted
    /// formats. Every constant string the first argument's type holds is
    /// checked, each malformed one reported with PHP's own exception
    /// message (PHPStan's `DateIntervalInstantiationRule`); any other
    /// argument is left alone.
    pub(crate) fn check_date_interval_constructor(
        &mut self,
        arenas: &NodeArenas,
        class: &str,
        args: &[Arg],
        span: Span,
        locals: &Locals,
    ) {
        if class != "DateInterval" {
            return;
        }
        let Some(arg) = args.first() else { return };
        let Some(value) = arenas.expr(arg.value) else {
            return;
        };
        let ty = self.infer_expr(value, locals);
        for spec in ty.constant_strings_text() {
            if is_valid_date_interval_spec(&spec) {
                continue;
            }
            report!(self, format!(
                    "Instantiating DateInterval with {spec} produces an error: Unknown or bad format ({spec})"
                ),
                span,
                5,
                "new.dateInterval",
            );
        }
    }
}

/// PHP accepts exactly two, wholly different spec grammars - the
/// familiar letter-designator duration form, or a fixed-width ISO-8601
/// extended date-time form reused as a duration. Leading/trailing
/// whitespace is tolerated (`" P1Y"`/`"P1Y "` both succeed); nothing
/// else is.
pub(crate) fn is_valid_date_interval_spec(spec: &str) -> bool {
    let s = spec.trim();
    is_valid_designator_form(s) || is_valid_iso_datetime_form(s)
}

/// `P[n]Y[n]M[n]W[n]D[T[n]H[n]M[n]S]` - every designator is optional,
/// but present designators must appear in exactly this relative order
/// with no repeats (`P1Y2M3D` succeeds, `P1D2Y` and `P1Y1Y` both throw),
/// matching `consume_designator_groups`'s own "search only forward from the
/// last matched position" shape encodes that directly. `T`'s own
/// presence is optional, but if present, at least one time designator
/// must follow it (`P1DT` throws) - and if absent, the date part must
/// have matched at least one designator itself (bare `P`/`P1`/`PY` all
/// throw). Case-sensitive throughout
/// (`strip_prefix('P')` alone already rejects a lowercase `p1y`).
fn is_valid_designator_form(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('P') else {
        return false;
    };
    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let Some(date_matched) = consume_designator_groups(date_part, &['Y', 'M', 'W', 'D']) else {
        return false;
    };
    match time_part {
        None => date_matched,
        Some(t) => matches!(consume_designator_groups(t, &['H', 'M', 'S']), Some(true)),
    }
}

/// Consumes `<digits><letter>` groups from `s`, where each `letter` must
/// be drawn from `designators`, strictly in that relative order (each
/// usable at most once) - real PHP's own grammar:
/// `P1D2Y` throws (D before Y), `P1Y1Y` throws (Y repeated), `P1W2D`/
/// `P2M1W` succeed (W freely mixes with M/D, unlike strict ISO-8601)
/// while `P1D2W`/`P1W2M` (W out of its own relative slot) throw.
/// Returns `Some(true)` if the whole string was consumed and at least
/// one group matched, `Some(false)` if the whole string was consumed
/// with no group at all (a legally empty part - the date part of
/// `PT1H`, or, before this fn is even asked, the time part when `T` is
/// altogether absent), or `None` if any prefix of `s` doesn't fit this
/// shape (no leading digits, an unrecognized/out-of-order/repeated
/// designator letter, a non-integer like `1.5` or `-1`, ...).
fn consume_designator_groups(s: &str, designators: &[char]) -> Option<bool> {
    let mut remaining = s;
    let mut next_idx = 0;
    let mut matched_any = false;
    while !remaining.is_empty() {
        let digit_len = remaining.bytes().take_while(u8::is_ascii_digit).count();
        if digit_len == 0 {
            return None;
        }
        let designator = remaining[digit_len..].chars().next()?;
        let offset = designators[next_idx..]
            .iter()
            .position(|&d| d == designator)?;
        next_idx += offset + 1;
        matched_any = true;
        remaining = &remaining[digit_len + designator.len_utf8()..];
    }
    Some(matched_any)
}

/// `P<yyyy>-<mm>-<dd>T<hh>:<mi>:<ss>` - every digit group's width is
/// fixed and the whole `T...` time suffix is mandatory
/// (`P2018-01-01`, with no time suffix, throws; `P01-02-03T04:05:06`,
/// with a 2-digit year, throws too).
fn is_valid_iso_datetime_form(s: &str) -> bool {
    const PATTERN: &[u8] = b"Pdddd-dd-ddTdd:dd:dd";
    let bytes = s.as_bytes();
    bytes.len() == PATTERN.len()
        && PATTERN.iter().zip(bytes.iter()).all(|(&p, &b)| {
            if p == b'd' {
                b.is_ascii_digit()
            } else {
                p == b
            }
        })
}
