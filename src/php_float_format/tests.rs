//! Every case here was checked against a live PHP 8.5 interpreter
//! (`php -r 'echo (string) $v;'`) before being pinned - see this module's
//! own doc comment.

use super::format_php_float;

// `3.14159265358979` below is a deliberate, real PHP-verified sample
// value (one digit short of `f64::consts::PI`, not a stand-in for it) -
// not clippy's suggested `std::f64::consts::PI`.
#[allow(clippy::approx_constant)]
#[test]
fn matches_real_php_output_for_the_confirmed_sample_values() {
    let cases: &[(f64, &str)] = &[
        (0.1 + 0.2, "0.3"),
        (1.5, "1.5"),
        (1.0, "1"),
        (100.0, "100"),
        (0.0001, "0.0001"),
        (0.00001, "1.0E-5"),
        (1e20, "1.0E+20"),
        (1e21, "1.0E+21"),
        (123456789012345.0, "1.2345678901234E+14"),
        (1234567890123456.0, "1.2345678901235E+15"),
        (0.1, "0.1"),
        (1e14, "1.0E+14"),
        (1e100, "1.0E+100"),
        (-1.5, "-1.5"),
        (3.14159265358979, "3.1415926535898"),
        (0.0, "0"),
        (-1.0, "-1"),
        (1e13, "10000000000000"),
        (-0.0001, "-0.0001"),
        (-0.00001, "-1.0E-5"),
        (9999999999999.0, "9999999999999"),
        (99999999999999.0, "99999999999999"),
        (999999999999999.0, "1.0E+15"),
        (5.0, "5"),
        (-5.0, "-5"),
        (2.5e-300, "2.5E-300"),
        (1e-300, "1.0E-300"),
        (123.456, "123.456"),
        (-0.30000000000000004, "-0.3"),
        (9.5e-5, "9.5E-5"),
    ];
    for (v, expected) in cases {
        assert_eq!(format_php_float(*v), *expected, "input: {v:?}");
    }
}

#[test]
fn negative_zero_keeps_its_own_sign_unlike_a_plain_positive_zero() {
    assert_eq!(format_php_float(0.0), "0");
    assert_eq!(format_php_float(-0.0), "-0");
}

#[test]
fn nan_and_infinities_use_php_s_own_case_sensitive_spellings() {
    assert_eq!(format_php_float(f64::NAN), "NAN");
    assert_eq!(format_php_float(f64::INFINITY), "INF");
    assert_eq!(format_php_float(f64::NEG_INFINITY), "-INF");
}
