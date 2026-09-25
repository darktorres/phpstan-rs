//! `SimpleXMLElement`'s own construction well-formedness checking. Real
//! PHPStan's `SimpleXMLElementConstructorThrowTypeExtension` needs to
//! know whether a literal XML string argument is well-formed before
//! `new SimpleXMLElement($xml)` would throw. Unlike `DateInterval`/
//! `DOMDocument::createElement()`'s own small, closed-form grammars,
//! this needs a real (if well-formedness-only) XML parser - `roxmltree`
//! is a minimal, read-only, pure-Rust parser with no C toolchain
//! requirement, strictly cheaper to add than the C dependency this
//! project already accepts for its PCRE work.

use crate::checker::Checker;
use syntax::ast::unwrap_grouping;
use syntax::ast::{Arg, ExprKind, Literal};
use syntax::token::Span;

impl<'a> Checker<'a> {
    /// `new SimpleXMLElement($xml)` - real PHP throws a plain `Exception`
    /// ("String could not be parsed as XML") when `$xml` isn't
    /// well-formed. Zero-arg/wrong-arity calls are left to the existing
    /// `arguments.count` check.
    pub(crate) fn check_simplexml_element_constructor(
        &mut self,
        class: &str,
        args: &[Arg],
        span: Span,
        label: &str,
    ) {
        if class != "SimpleXMLElement" {
            return;
        }
        let Some(arg) = args.first() else { return };
        if arg.name.is_some() || arg.spread {
            return;
        }
        let ExprKind::Literal(Literal::String(xml)) = &unwrap_grouping(&arg.value).kind else {
            return;
        };
        if let Some(detail) = is_well_formed_xml(xml) {
            self.report(
                label.to_string(),
                format!("SimpleXMLElement constructor argument is not well-formed XML ({detail}) and will throw at runtime"),
                span,
                0,
                "argument.type",
            );
        }
    }

    /// `simplexml_load_string($xml)` - unlike the constructor, real PHP
    /// never throws here: a malformed argument makes it return `false`
    /// with a warning instead. Still worth flagging on its own merits
    /// (an `argument.type` diagnostic, not a `@throws`-tag contribution -
    /// this function never touches `dynamic_new_throw_type`/`dynamic_
    /// method_throw_type`/`dynamic_function_throw_type` at all, since it
    /// has no throw type to contribute).
    pub(crate) fn check_simplexml_load_string_call(
        &mut self,
        args: &[Arg],
        span: Span,
        label: &str,
    ) {
        let Some(arg) = args.first() else { return };
        if arg.name.is_some() || arg.spread {
            return;
        }
        let ExprKind::Literal(Literal::String(xml)) = &unwrap_grouping(&arg.value).kind else {
            return;
        };
        if let Some(detail) = is_well_formed_xml(xml) {
            self.report(
                label.to_string(),
                format!("simplexml_load_string() argument is not well-formed XML ({detail}) and will return false at runtime"),
                span,
                0,
                "argument.type",
            );
        }
    }
}

/// Returns `None` when `xml` is well-formed, `Some(detail)` (the parse
/// error's own message) otherwise. Mirrors `pcre_validate::validate_
/// pattern`'s own `Option<String>` "detail or valid" shape.
pub(crate) fn is_well_formed_xml(xml: &str) -> Option<String> {
    roxmltree::Document::parse(xml).err().map(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::is_well_formed_xml;

    #[test]
    fn a_well_formed_minimal_document_is_accepted() {
        assert!(is_well_formed_xml("<a><b></b></a>").is_none());
    }

    #[test]
    fn a_mismatched_closing_tag_is_rejected() {
        assert!(is_well_formed_xml("<a><b></a>").is_some());
    }

    #[test]
    fn an_unclosed_tag_is_rejected() {
        assert!(is_well_formed_xml("<a><b>").is_some());
    }

    #[test]
    fn an_empty_string_is_rejected() {
        assert!(is_well_formed_xml("").is_some());
    }
}
