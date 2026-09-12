//! Phase 46: `regexp.pattern` - a malformed PCRE pattern literal, real
//! PHPStan's own always-on `RegularExpressionPatternRule`
//! (`#[RegisteredRule(level: 0)]`). Real PHPStan doesn't reimplement PCRE
//! grammar checking either - it validates by literally calling
//! `Nette\Utils\Strings::match('', $pattern)` (a thin wrapper over real
//! `preg_match`) and surfacing whatever the live PHP/PCRE2 runtime says.
//! This project can't call real PHP at analysis time, so it re-derives
//! the same two layers PHP's own C engine actually performs before ever
//! reaching a match attempt:
//!
//! 1. Delimiter/modifier validation (pure Rust, no PCRE dependency) -
//!    wording confirmed against a live PHP 8.5 interpreter (`php -r`),
//!    not version-gated: this project has no older PHP interpreter
//!    available to verify any earlier wording against, and real PHPStan
//!    itself doesn't hardcode historical wording either (it always
//!    surfaces whatever the live runtime it's running under says).
//! 2. A real PCRE2 compile via the low-level `pcre2-sys` FFI crate - not
//!    the higher-level `pcre2` crate's safe `bytes::RegexBuilder`, which
//!    has no way to set PHP's `J` modifier (`PCRE2_DUPNAMES`): a real,
//!    legal PHP pattern like `/(?<a>x)|(?<a>y)/J` would otherwise always
//!    be wrongly reported invalid - confirmed live (`/(?<a>x)|(?<a>y)/`
//!    without `J` fails to compile with "two named subpatterns have the
//!    same name (PCRE2_DUPNAMES not set)"; with `J` it compiles fine).
//!
//! Feature-gated on `pcre-validate` (on by default, in `Cargo.toml`'s own
//! `default` feature list) - the one place this project links a C
//! dependency at all. `cargo build --no-default-features` keeps the rest
//! of the crate 100% pure Rust, at the cost of only this one diagnostic
//! (`validate_pattern` still runs its own pure-Rust delimiter/modifier
//! checks in that build - only the real PCRE2 compile step is skipped,
//! deliberately never flagging a pattern that passes those, matching
//! this project's "can't verify, don't guess wrong" convention).

use crate::regex_groups::split_delimiters_and_modifiers;

/// Every modifier letter real PHP 8.5 accepts, confirmed empirically
/// (`php -r` probing every ASCII letter one at a time against a trivial
/// pattern) rather than assumed from the manual's own documented subset,
/// which turned out to be incomplete - `n` (`PCRE2_NO_AUTO_CAPTURE`,
/// confirmed behaviorally: unnamed groups stop capturing under it) and
/// `r` (accepted, no observable compile-time or match-time effect this
/// investigation could pin down) are both real, accepted modifiers PHP's
/// own manual page doesn't list alongside the more common ones.
const VALID_MODIFIERS: &str = "imnrsuxADJSUX";

/// Validates one PHP PCRE pattern literal (the raw, still-delimited
/// string exactly as written, e.g. `"/foo/i"`) - `None` when it's valid
/// (or empty/unparseable in a way this function declines to guess about),
/// `Some(detail)` with a human-readable problem description otherwise.
pub(crate) fn validate_pattern(pattern_literal: &str) -> Option<String> {
    let open = pattern_literal.chars().next()?;
    if open.is_alphanumeric() || open == '\\' || open == '\0' {
        return Some("Delimiter must not be alphanumeric, backslash, or NUL byte".to_string());
    }
    let Some((body, modifiers)) = split_delimiters_and_modifiers(pattern_literal) else {
        // `split_delimiters_and_modifiers` itself already knows when it
        // fails to find a closing delimiter - surfaced here in this
        // project's own words (house style, not a verbatim PHP quote:
        // the real wording lives in PHP's own `ext/pcre` C source, not
        // vendored here to match byte-for-byte).
        return Some(format!(
            "no ending matching delimiter '{}' found",
            closing_delimiter(open)
        ));
    };
    for m in modifiers.chars() {
        if !VALID_MODIFIERS.contains(m) {
            return Some(format!("Unknown modifier '{m}'"));
        }
    }
    compile_check(body, modifiers)
}

fn closing_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '{' => '}',
        '[' => ']',
        '<' => '>',
        c => c,
    }
}

#[cfg(feature = "pcre-validate")]
fn compile_check(body: &str, modifiers: &str) -> Option<String> {
    ffi::compile_check(body, modifiers)
}

#[cfg(not(feature = "pcre-validate"))]
fn compile_check(_body: &str, _modifiers: &str) -> Option<String> {
    None
}

#[cfg(feature = "pcre-validate")]
mod ffi {
    use pcre2_sys::{
        PCRE2_ANCHORED, PCRE2_CASELESS, PCRE2_DOLLAR_ENDONLY, PCRE2_DOTALL, PCRE2_DUPNAMES,
        PCRE2_EXTENDED, PCRE2_MULTILINE, PCRE2_NO_AUTO_CAPTURE, PCRE2_UNGREEDY, PCRE2_UTF,
        pcre2_code_free_8, pcre2_compile_8, pcre2_get_error_message_8,
    };
    use std::os::raw::c_int;

    /// Maps each of `VALID_MODIFIERS`'s own letters to its real PCRE2
    /// compile-time option bit - `S`/`X`/`r` contribute nothing (see
    /// `VALID_MODIFIERS`'s own doc comment: real, accepted PHP modifiers
    /// with no corresponding `pcre2_compile` option this investigation
    /// could identify - `S` is PCRE1-era `pcre_study()` optimization
    /// advice with no PCRE2 compile-option equivalent, `X`'s old
    /// `PCRE_EXTRA` semantics are simply PCRE2's own default behavior
    /// now, and `r`'s own effect wasn't pinned down as changing compile
    /// validity in any way this check needs to represent).
    fn modifier_options(modifiers: &str) -> u32 {
        let mut opts = 0u32;
        for m in modifiers.chars() {
            opts |= match m {
                'i' => PCRE2_CASELESS,
                'm' => PCRE2_MULTILINE,
                's' => PCRE2_DOTALL,
                'x' => PCRE2_EXTENDED,
                'u' => PCRE2_UTF,
                'A' => PCRE2_ANCHORED,
                'D' => PCRE2_DOLLAR_ENDONLY,
                'J' => PCRE2_DUPNAMES,
                'U' => PCRE2_UNGREEDY,
                'n' => PCRE2_NO_AUTO_CAPTURE,
                _ => 0,
            };
        }
        opts
    }

    /// Compiles `body` (never executed - `pcre2_compile_8` alone, no
    /// `pcre2_match_8` call) against real PCRE2, returning `None` when it
    /// compiles cleanly or `Some(detail)` built from PCRE2's own real
    /// error code/offset otherwise.
    pub(super) fn compile_check(body: &str, modifiers: &str) -> Option<String> {
        let options = modifier_options(modifiers);
        let mut error_code: c_int = 0;
        let mut error_offset: usize = 0;
        // Safety: `body.as_ptr()`/`body.len()` describe a valid,
        // initialized byte slice for the duration of this call;
        // `error_code`/`error_offset` are valid `&mut` targets PCRE2
        // only ever writes through; the compile-context pointer is
        // null, which `pcre2_compile_8` documents as "use the default
        // compile context" - the resulting `code` is freed before
        // returning, and never used for anything beyond the null check
        // (this function never matches against it).
        let code = unsafe {
            pcre2_compile_8(
                body.as_ptr(),
                body.len(),
                options,
                &mut error_code,
                &mut error_offset,
                std::ptr::null_mut(),
            )
        };
        if !code.is_null() {
            unsafe {
                pcre2_code_free_8(code);
            }
            return None;
        }
        let mut buf = [0u8; 256];
        // Safety: `buf` is a valid, initialized, appropriately-sized
        // output buffer PCRE2 writes at most `buf.len()` bytes into.
        let len = unsafe { pcre2_get_error_message_8(error_code, buf.as_mut_ptr(), buf.len()) };
        let message = if len > 0 {
            String::from_utf8_lossy(&buf[..len as usize]).into_owned()
        } else {
            format!("PCRE2 compile error {error_code}")
        };
        Some(format!(
            "Compilation failed: {message} at offset {error_offset}"
        ))
    }
}

#[cfg(test)]
mod tests;
