//! The file's call-site scalar strictness, read from its `declare`
//! statements.

use crate::ast::{ExprKind, Literal, NodeArenas, StmtId, StmtKind};

/// The file's call-site scalar strictness, read from the leading file-level
/// `declare` statements. `None` means a directive in that prefix is malformed
/// or misplaced, so callers must avoid using it to narrow runtime behavior.
/// An absent directive is PHP's weak mode; other misplaced directives are
/// diagnosed by `check_declare_directives`.
pub fn strict_types_mode(arenas: &NodeArenas, program: &[StmtId]) -> Option<bool> {
    let mut mode = None;
    for (index, id) in program.iter().enumerate() {
        let is_first = index == 0;
        let stmt = arenas.stmt(*id)?;
        let StmtKind::Declare { directives, body } = &stmt.kind else {
            break;
        };
        for directive in arenas.declare_directives(directives.clone()) {
            if !directive.name.eq_ignore_ascii_case("strict_types") {
                continue;
            }
            if !is_first || body.is_some() || mode.is_some() {
                return None;
            }
            let value = arenas.expr(directive.value)?;
            let ExprKind::Literal(Literal::Int(value)) = value.kind else {
                return None;
            };
            match value {
                0 => mode = Some(false),
                1 => mode = Some(true),
                _ => return None,
            }
        }
        if body.is_some() {
            break;
        }
    }
    Some(mode.unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::strict_types_mode;

    fn mode(source: &str) -> Option<bool> {
        let parsed = crate::parse(source).expect("the source parses");
        strict_types_mode(&parsed.arenas, parsed.statements())
    }

    #[test]
    fn no_declare_is_weak_mode() {
        assert_eq!(mode("<?php echo 1;"), Some(false));
    }

    #[test]
    fn zero_is_weak_mode() {
        assert_eq!(mode("<?php declare(strict_types=0);"), Some(false));
    }

    #[test]
    fn one_is_strict_mode() {
        assert_eq!(mode("<?php declare(strict_types=1);"), Some(true));
    }

    #[test]
    fn the_name_is_case_insensitive() {
        assert_eq!(mode("<?php declare(STRICT_TYPES=1);"), Some(true));
    }

    #[test]
    fn a_strict_types_after_another_declare_is_not_a_mode() {
        assert_eq!(
            mode("<?php declare(ticks=1); declare(strict_types=1);"),
            None
        );
    }

    #[test]
    fn a_block_form_declare_is_not_a_mode() {
        assert_eq!(mode("<?php declare(strict_types=1) { echo 1; }"), None);
    }

    #[test]
    fn a_non_literal_value_is_not_a_mode() {
        assert_eq!(mode("<?php declare(strict_types=$x);"), None);
    }

    #[test]
    fn another_integer_is_not_a_mode() {
        assert_eq!(mode("<?php declare(strict_types=2);"), None);
    }

    #[test]
    fn a_second_strict_types_is_not_a_mode() {
        assert_eq!(
            mode("<?php declare(strict_types=1); declare(strict_types=0);"),
            None
        );
    }

    #[test]
    fn a_declare_after_a_statement_is_not_read() {
        // Not in the leading prefix, so the file is weak mode here; PHP
        // rejects the placement itself, and `check_declare_directives`
        // reports it.
        assert_eq!(mode("<?php echo 1; declare(strict_types=1);"), Some(false));
    }
}
