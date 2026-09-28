//! PHP's numeric strings: which strings `is_numeric()` accepts, and the
//! number arithmetic reads one as.

/// A numeric string's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhpNumber {
    Int(i64),
    Float(f64),
}

/// The whitespace PHP allows around a numeric string.
const WHITESPACE: &[u8] = b" \t\n\r\x0b\x0c";

/// The number PHP reads `bytes` as when they are a numeric string -
/// `[+-]` digits with an optional fraction and exponent, whitespace
/// around it allowed - or `None`. It is an int when written as one
/// (no `.` or exponent) that fits, a float otherwise.
pub fn numeric_value(bytes: &[u8]) -> Option<PhpNumber> {
    let start = bytes.iter().position(|b| !WHITESPACE.contains(b))?;
    let end = bytes.iter().rposition(|b| !WHITESPACE.contains(b))? + 1;
    let text = &bytes[start..end];
    let digits_from = |i: usize| {
        text[i.min(text.len())..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut i = usize::from(matches!(text.first(), Some(b'+' | b'-')));
    let int_digits = digits_from(i);
    i += int_digits;
    let mut written_as_int = true;
    let mut fraction_digits = 0;
    if text.get(i) == Some(&b'.') {
        written_as_int = false;
        fraction_digits = digits_from(i + 1);
        i += 1 + fraction_digits;
    }
    if int_digits + fraction_digits == 0 {
        return None;
    }
    if matches!(text.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(text.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let exponent_digits = digits_from(j);
        if exponent_digits == 0 {
            return None;
        }
        written_as_int = false;
        i = j + exponent_digits;
    }
    if i != text.len() {
        return None;
    }
    let text = std::str::from_utf8(text).ok()?;
    if written_as_int && let Ok(n) = text.parse::<i64>() {
        return Some(PhpNumber::Int(n));
    }
    text.parse::<f64>().ok().map(PhpNumber::Float)
}

/// PHP's `++` on a non-numeric, non-empty string: the last alphanumeric
/// run counts up with carry (`'a'` to `'b'`, `'Az'` to `'Ba'`, `'z9'` to
/// `'aa0'`), stopping at any other byte (`'a-'` stays `'a-'`).
pub(crate) fn increment_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut carry_with = None;
    for b in out.iter_mut().rev() {
        let (first, last) = match *b {
            b'a'..=b'z' => (b'a', b'z'),
            b'A'..=b'Z' => (b'A', b'Z'),
            b'0'..=b'9' => (b'0', b'9'),
            _ => {
                carry_with = None;
                break;
            }
        };
        if *b == last {
            *b = first;
            carry_with = Some(if first == b'0' { b'1' } else { first });
        } else {
            *b += 1;
            carry_with = None;
            break;
        }
    }
    if let Some(lead) = carry_with {
        out.insert(0, lead);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_values_follow_php() {
        use PhpNumber::*;
        assert_eq!(numeric_value(b"5"), Some(Int(5)));
        assert_eq!(numeric_value(b" 5"), Some(Int(5)));
        assert_eq!(numeric_value(b"5 \n"), Some(Int(5)));
        assert_eq!(numeric_value(b"+5"), Some(Int(5)));
        assert_eq!(numeric_value(b"-0"), Some(Int(0)));
        assert_eq!(numeric_value(b"5.5"), Some(Float(5.5)));
        assert_eq!(numeric_value(b"5."), Some(Float(5.0)));
        assert_eq!(numeric_value(b".5"), Some(Float(0.5)));
        assert_eq!(numeric_value(b"1e3"), Some(Float(1000.0)));
        assert_eq!(
            numeric_value(b"9223372036854775808"),
            Some(Float(9_223_372_036_854_775_808.0))
        );
        for not in [
            &b""[..],
            b" ",
            b".",
            b"1e",
            b"0x1",
            b"5abc",
            b"1 5",
            b"\xc2\xa01",
            b"e5",
            b"+",
        ] {
            assert_eq!(
                numeric_value(not),
                None,
                "{:?}",
                String::from_utf8_lossy(not)
            );
        }
    }

    #[test]
    fn string_increment_carries_like_php() {
        for (from, to) in [
            ("a", "b"),
            ("z", "aa"),
            ("Az", "Ba"),
            ("zz", "aaa"),
            ("Zz", "AAa"),
            ("a9", "b0"),
            ("z9", "aa0"),
            ("9z", "10a"),
            ("a-", "a-"),
            ("a b", "a c"),
            ("a-z", "a-a"),
            ("0x1", "0x2"),
        ] {
            assert_eq!(increment_string(from.as_bytes()), to.as_bytes(), "{from}");
        }
    }
}
