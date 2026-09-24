//! `DOMDocument::createElement()`'s own tag-name validation. Real
//! PHPStan's `DomDocumentCreateElementDynamicThrowTypeExtension`
//! validates a single tag-name string against the finite W3C XML 1.0
//! `Name` production before `createElement()` would throw `DOMException`,
//! never parsing an XML document at all - structurally identical in
//! shape to `DateInterval`'s own duration-spec grammar
//! (`date_interval.rs`), not "full XML-content validation." A colon is
//! accepted as a literal `Name` character (`createElement('a:b')`
//! succeeds) - the base, colon-permissive `Name` grammar, not the
//! namespace-restricted `NCName` one.

use crate::ast::{Arg, ExprKind, Literal};
use crate::checker::Checker;
use crate::checker::expr::unwrap_grouping;
use crate::token::Span;

impl<'a> Checker<'a> {
    /// `(new DOMDocument())->createElement($name)` - real PHP throws
    /// `DOMException` when `$name` isn't a valid XML `Name`. Zero-arg
    /// calls are left entirely to the existing `arguments.count` check
    /// (the real stub's `$localName` parameter has no default) rather
    /// than special-cased here.
    pub(crate) fn check_dom_create_element(
        &mut self,
        class: &str,
        method: &str,
        args: &[Arg],
        span: Span,
        label: &str,
    ) {
        if class != "DOMDocument" || method != "createElement" {
            return;
        }
        let [arg] = args else { return };
        if arg.name.is_some() || arg.spread {
            return;
        }
        let ExprKind::Literal(Literal::String(name)) = &unwrap_grouping(&arg.value).kind else {
            return;
        };
        if is_valid_xml_name(name) {
            return;
        }
        self.report(
            label.to_string(),
            format!("DOMDocument::createElement() name '{name}' is not a valid XML element name and will throw at runtime"),
            span,
            0,
            "argument.type",
        );
    }
}

/// The W3C XML 1.0 (Fifth Edition) §2.3 `Name` production - the grammar
/// `createElement` (`ext-dom`/libxml) actually validates against, not
/// the namespace-restricted `NCName` grammar (a colon is a valid,
/// literal `NameChar`/`NameStartChar`).
pub(crate) fn is_valid_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if is_name_start_char(c) => {}
        _ => return false,
    }
    chars.all(is_name_char)
}

fn is_name_start_char(c: char) -> bool {
    c == ':'
        || c == '_'
        || c.is_ascii_alphabetic()
        || matches!(c as u32,
            0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF | 0x370..=0x37D | 0x37F..=0x1FFF
            | 0x200C..=0x200D | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

fn is_name_char(c: char) -> bool {
    is_name_start_char(c)
        || c == '-'
        || c == '.'
        || c.is_ascii_digit()
        || c as u32 == 0xB7
        || matches!(c as u32, 0x0300..=0x036F | 0x203F..=0x2040)
}

#[cfg(test)]
mod tests {
    use super::is_valid_xml_name;

    #[test]
    fn ordinary_ascii_tag_names_are_valid() {
        assert!(is_valid_xml_name("div"));
        assert!(is_valid_xml_name("my-tag"));
        assert!(is_valid_xml_name("my.tag"));
        assert!(is_valid_xml_name("_leading_underscore"));
        assert!(is_valid_xml_name("a1"));
    }

    #[test]
    fn a_colon_is_accepted_as_a_literal_name_character() {
        // `createElement('a:b')` succeeds - this is the base Name
        // grammar, not NCName.
        assert!(is_valid_xml_name("a:b"));
    }

    #[test]
    fn empty_string_is_invalid() {
        assert!(!is_valid_xml_name(""));
    }

    #[test]
    fn a_leading_digit_or_hyphen_or_period_is_invalid() {
        assert!(!is_valid_xml_name("1tag"));
        assert!(!is_valid_xml_name("-tag"));
        assert!(!is_valid_xml_name(".tag"));
    }

    #[test]
    fn whitespace_or_angle_brackets_or_ampersand_are_invalid() {
        assert!(!is_valid_xml_name("has space"));
        assert!(!is_valid_xml_name("<tag>"));
        assert!(!is_valid_xml_name("a&b"));
        assert!(!is_valid_xml_name("a\"b"));
    }
}
