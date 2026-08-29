use std::collections::HashMap;

/// A parsed subset of a `/** ... */` doc comment: just the tags this
/// prototype's type checker can use (`@param`, `@return`, `@var`). Types are
/// kept as raw strings here, resolved through `types::parse_type_string`
/// only where a caller actually needs a `Type` - this module doesn't depend
/// on `types` at all, so it can't unintentionally do (or fail to do) any
/// resolution work of its own.
///
/// Deliberately does not parse generics, array shapes, conditional return
/// types, or `@template` - those need a real second type-string grammar,
/// not this line-oriented tag scanner. `@param`/`@return`/`@var` cover
/// PHPDoc's single biggest practical win with the least machinery: adding a
/// type to something a native hint can't express (an untyped param/property,
/// or PHP versions without union types).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocBlock {
    pub params: HashMap<String, String>,
    pub return_type: Option<String>,
    pub var_type: Option<String>,
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
                let mut rp = rest.splitn(2, char::is_whitespace);
                let ty = rp.next().unwrap_or("");
                let remainder = rp.next().unwrap_or("").trim();
                let var_token = remainder.split_whitespace().next().unwrap_or("");
                if let Some(name) = var_token.strip_prefix('$')
                    && !ty.is_empty()
                {
                    block.params.insert(name.to_string(), ty.to_string());
                }
            }
            "return" => {
                if let Some(ty) = rest.split_whitespace().next() {
                    block.return_type = Some(ty.to_string());
                }
            }
            "var" => {
                if let Some(ty) = rest.split_whitespace().next() {
                    block.var_type = Some(ty.to_string());
                }
            }
            _ => {}
        }
    }

    block
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
}
