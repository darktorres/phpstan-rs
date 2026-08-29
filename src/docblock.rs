use std::collections::HashMap;

/// A parsed subset of a `/** ... */` doc comment: just the tags this
/// prototype's type checker can use (`@param`, `@return`, `@var`). Types are
/// kept as raw strings here, resolved through `types::parse_type_string`
/// only where a caller actually needs a `Type` - this module doesn't depend
/// on `types` at all, so it can't unintentionally do (or fail to do) any
/// resolution work of its own.
///
/// Deliberately does not parse conditional return types or `@template` -
/// those need real per-call-site type parameter binding, not just parsing.
/// `@param`/`@return`/`@var` cover PHPDoc's single biggest practical win
/// with the least machinery: adding a type to something a native hint can't
/// express (an untyped param/property, a native `array` refined to a shape
/// or generic, or PHP versions without union types).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocBlock {
    pub params: HashMap<String, String>,
    pub return_type: Option<String>,
    pub var_type: Option<String>,
    // `@template T` names (a bound, `@template T of SomeClass`, is parsed
    // past but not recorded - nothing consumes it yet).
    pub templates: Vec<String>,
}

pub fn parse(raw: &str) -> DocBlock {
    let mut block = DocBlock::default();

    let inner = raw.trim().trim_start_matches("/**").trim_end_matches("*/");

    for raw_line in inner.lines() {
        let line = raw_line.trim();
        let line = line.strip_prefix('*').map(str::trim).unwrap_or(line);
        let Some(tag_body) = line.strip_prefix('@') else {
            continue;
        };
        let mut parts = tag_body.splitn(2, char::is_whitespace);
        let tag = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();

        match tag {
            "param" => {
                // Convention is `Type $name description...`.
                let ty = take_type_token(rest);
                let remainder = rest[ty.len()..].trim_start();
                let var_token = remainder.split_whitespace().next().unwrap_or("");
                if let Some(name) = var_token.strip_prefix('$')
                    && !ty.is_empty()
                {
                    block.params.insert(name.to_string(), ty.to_string());
                }
            }
            "return" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.return_type = Some(ty.to_string());
                }
            }
            "var" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.var_type = Some(ty.to_string());
                }
            }
            "template" => {
                if let Some(name) = rest.split_whitespace().next() {
                    block.templates.push(name.to_string());
                }
            }
            _ => {}
        }
    }

    block
}

/// Takes the leading type expression off `s`, stopping at the first
/// whitespace that occurs outside any `<>`/`{}`/`()`/`[]` nesting - a plain
/// `split_whitespace().next()` would wrongly cut an array shape or generic
/// off at its first internal space (`array{count: int, ...}`).
fn take_type_token(s: &str) -> &str {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => return &s[..i],
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_param_and_return() {
        let block = parse(
            r#"/**
             * Adds one to a number.
             *
             * @param int $x the input
             * @return int
             */"#,
        );
        assert_eq!(block.params.get("x"), Some(&"int".to_string()));
        assert_eq!(block.return_type, Some("int".to_string()));
    }

    #[test]
    fn parses_var() {
        let block = parse("/** @var string|null */");
        assert_eq!(block.var_type, Some("string|null".to_string()));
    }

    #[test]
    fn parses_multiple_params() {
        let block = parse(
            r#"/**
             * @param int $a
             * @param string $b
             */"#,
        );
        assert_eq!(block.params.get("a"), Some(&"int".to_string()));
        assert_eq!(block.params.get("b"), Some(&"string".to_string()));
    }

    #[test]
    fn ignores_unknown_tags_and_prose() {
        let block = parse(
            r#"/**
             * This function does a thing.
             *
             * @throws \RuntimeException
             * @deprecated use something else
             */"#,
        );
        assert!(block.params.is_empty());
        assert!(block.return_type.is_none());
        assert!(block.var_type.is_none());
    }

    #[test]
    fn single_line_doc_comment() {
        let block = parse("/** @return bool */");
        assert_eq!(block.return_type, Some("bool".to_string()));
    }

    #[test]
    fn return_type_with_internal_spaces_is_not_truncated() {
        let block = parse("/** @return array{count: int, first: string} */");
        assert_eq!(
            block.return_type,
            Some("array{count: int, first: string}".to_string())
        );
    }

    #[test]
    fn param_type_with_internal_spaces_still_finds_the_variable_name() {
        let block = parse("/** @param array{count: int, first: string} $x the input */");
        assert_eq!(
            block.params.get("x"),
            Some(&"array{count: int, first: string}".to_string())
        );
    }

    #[test]
    fn generic_type_with_comma_and_space_is_not_truncated() {
        let block = parse("/** @param array<int, User> $users */");
        assert_eq!(
            block.params.get("users"),
            Some(&"array<int, User>".to_string())
        );
    }

    #[test]
    fn parses_template_name() {
        let block = parse(
            r#"/**
             * @template T
             * @param T $value
             * @return T
             */"#,
        );
        assert_eq!(block.templates, vec!["T".to_string()]);
        assert_eq!(block.params.get("value"), Some(&"T".to_string()));
        assert_eq!(block.return_type, Some("T".to_string()));
    }

    #[test]
    fn parses_bounded_template_name_ignoring_the_bound() {
        let block = parse("/** @template T of \\Countable */");
        assert_eq!(block.templates, vec!["T".to_string()]);
    }
}
