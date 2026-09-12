//! Phase 75: PHP's own float-to-string cast/concatenation algorithm.
//!
//! Empirically confirmed against a live PHP 8.5 interpreter (`php -r`),
//! not derived from the `serialize_precision=-1` shortest-round-trip path
//! that's `var_export()`'s own, separate, algorithm: `(string)(0.1+0.2)`
//! is `"0.3"` while `var_export(0.1+0.2)` is `"0.30000000000000004"`, and
//! `(string)1.0` is `"1"`, not `"1.0"`. Every sampled value matched
//! `sprintf("%.14G", $v)` character-for-character - a fixed 14-
//! significant-digit `%G`-style formatter (scientific notation exactly
//! when the exponent is `< -4` or `>= 14`, at most 14 significant digits,
//! trailing zeros stripped), driven by the `precision` ini directive
//! (default 14), not a shortest-round-trip encoder. One real difference
//! from bare C `%G`: PHP's own implementation never drops the decimal
//! point entirely in scientific notation, even when every digit after
//! the leading one strips away (`sprintf("%.14G", 1e20)` is `"1.0E+20"`,
//! not `"1E+20"`) - confirmed directly, not assumed from the C standard.
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
