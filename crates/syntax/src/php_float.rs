//! A float as PHP source writes it.

/// A float as `var_export()` writes it: the shortest digits that read
/// back as the same value, always with a decimal point (`1.0`, `-0.0`),
/// in `E` notation when the decimal exponent is below -4 or 17 or more
/// (`1.0E-5`, `1.8446744073709552E+19`).
pub fn var_export_float(f: f64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_format_matches_var_export() {
        for (v, expected) in [
            (1001.0, "1001.0"),
            (1e16, "10000000000000000.0"),
            (1e17, "1.0E+17"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (1.5e300, "1.5E+300"),
            (-2.5, "-2.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (18446744073709551616.0, "1.8446744073709552E+19"),
            (-0.0, "-0.0"),
            (1.5e-7, "1.5E-7"),
        ] {
            assert_eq!(var_export_float(v), expected, "input: {v:?}");
        }
    }
}
