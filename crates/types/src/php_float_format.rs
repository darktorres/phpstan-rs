//! PHP's own float-to-string cast/concatenation algorithm.
//!
//! This is a fixed 14-significant-digit `%G`-style formatter
//! (scientific notation exactly when the exponent is `< -4` or `>= 14`,
//! at most 14 significant digits, trailing zeros stripped), driven by
//! the `precision` ini directive (default 14), not a shortest-round-trip
//! encoder - `serialize_precision=-1`'s shortest-round-trip path is
//! `var_export()`'s own, separate algorithm: `(string)(0.1+0.2)` is
//! `"0.3"` while `var_export(0.1+0.2)` is `"0.30000000000000004"`, and
//! `(string)1.0` is `"1"`, not `"1.0"`. Matches `sprintf("%.14G", $v)`
//! character-for-character, with one real difference from bare C `%G`:
//! PHP's own implementation never drops the decimal point entirely in
//! scientific notation, even when every digit after the leading one
//! strips away (`sprintf("%.14G", 1e20)` is `"1.0E+20"`, not
//! `"1E+20"`).
//!
//! Unlike real `ConstantFloatType::toString()` (which unions `'0'`/`'-0'`
//! for an exactly-zero value, since PHP's own `===` can't distinguish
//! `0.0` from `-0.0` at that level), this function operates on a real,
//! already-computed `f64` whose sign bit Rust preserves - `f.is_sign_
//! negative()` answers the same question precisely, so callers never
//! need the union-of-both-signs workaround.

pub(crate) fn format_php_float(f: f64) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "INF".to_string()
        } else {
            "-INF".to_string()
        };
    }
    if f == 0.0 {
        return if f.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    const SIGNIFICANT_DIGITS: usize = 14;
    let negative = f.is_sign_negative();
    let abs = f.abs();

    // `{:.*e}` with `SIGNIFICANT_DIGITS - 1` digits after the decimal
    // point gives exactly `SIGNIFICANT_DIGITS` significant digits total
    // (the one digit before the point, plus these) in scientific
    // notation - correctly-rounded by Rust's own float formatter, not a
    // hand-rolled rounding step.
    let sci = format!("{:.*e}", SIGNIFICANT_DIGITS - 1, abs);
    let (mantissa, exp_str) = sci
        .split_once('e')
        .expect("Rust's `e` scientific-notation formatting always includes an exponent");
    let exponent: i32 = exp_str
        .parse()
        .expect("Rust's own exponent formatting is always a valid integer");

    // `mantissa` is `"D.DDDDDDDDDDDDD"` (no sign - `abs` is
    // non-negative) - strip the decimal point to get the raw
    // `SIGNIFICANT_DIGITS` digits.
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();

    let body = if exponent < -4 || exponent >= SIGNIFICANT_DIGITS as i32 {
        // Scientific notation - trailing zeros stripped from the
        // mantissa, but always keeping at least one fractional digit
        // (PHP's own real difference from bare C `%G`, confirmed above).
        let frac = digits[1..].trim_end_matches('0');
        let frac = if frac.is_empty() { "0" } else { frac };
        format!(
            "{}.{frac}E{}{}",
            &digits[..1],
            if exponent >= 0 { "+" } else { "-" },
            exponent.abs()
        )
    } else if exponent < 0 {
        // `0.00DDDDDDDDDDDDD` form - `-exponent - 1` leading zeros after
        // the decimal point, then the digits, trailing zeros stripped.
        let leading_zeros = "0".repeat((-exponent - 1) as usize);
        let frac = format!("{leading_zeros}{digits}");
        let frac = frac.trim_end_matches('0');
        let frac = if frac.is_empty() { "0" } else { frac };
        format!("0.{frac}")
    } else {
        // `DDDD.DDDDDDD` form (or a bare integer once the fractional
        // part is all zeros) - the decimal point sits `exponent + 1`
        // digits in from the left.
        let point = (exponent + 1) as usize;
        let (int_part, frac_part) = if point >= digits.len() {
            (
                format!("{digits}{}", "0".repeat(point - digits.len())),
                String::new(),
            )
        } else {
            (digits[..point].to_string(), digits[point..].to_string())
        };
        let frac_part = frac_part.trim_end_matches('0');
        if frac_part.is_empty() {
            int_part
        } else {
            format!("{int_part}.{frac_part}")
        }
    };

    if negative { format!("-{body}") } else { body }
}

#[cfg(test)]
mod tests;

/// A float as `var_export()` writes it: the shortest digits that read
/// back as the same value, always with a decimal point (`1.0`, `-0.0`),
/// in `E` notation when the decimal exponent is below -4 or 17 or more
/// (`1.0E-5`, `1.8446744073709552E+19`).
pub(crate) fn format_php_float_exact(f: f64) -> String {
    if f.is_nan() {
        return "NAN".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 { "INF" } else { "-INF" }.to_string();
    }
    let sign = if f.is_sign_negative() { "-" } else { "" };
    // `{:e}` gives the shortest round-trip digits: `d.ddde<exp>`.
    let sci = format!("{:e}", f.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("{:e} has an exponent");
    let exponent: i32 = exponent.parse().expect("{:e}'s exponent is an integer");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if !(-4..17).contains(&exponent) {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        let exp_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{first}.{rest}E{exp_sign}{}", exponent.abs());
    }
    let point = exponent + 1;
    let (int_part, frac_part) = if point <= 0 {
        (
            "0".to_string(),
            format!("{}{digits}", "0".repeat((-point) as usize)),
        )
    } else if point as usize >= digits.len() {
        (
            format!("{digits}{}", "0".repeat(point as usize - digits.len())),
            String::new(),
        )
    } else {
        let (i, fr) = digits.split_at(point as usize);
        (i.to_string(), fr.to_string())
    };
    let frac_part = if frac_part.is_empty() {
        "0".to_string()
    } else {
        frac_part
    };
    format!("{sign}{int_part}.{frac_part}")
}
