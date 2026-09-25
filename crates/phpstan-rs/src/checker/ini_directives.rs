//! `ini_get()`/`ini_set()`/`ini_alter()`/`ini_restore()`/`get_cfg_var()`
//! naming a directive the configured PHP version no longer has
//! (`ini.removed`: the call returns `false` and changes nothing) or has
//! deprecated (`ini.deprecated`, with `check_deprecated_usage`). Some
//! directives are deprecated only to change, so reading them is fine.

use crate::checker::{Checker, Locals};
use crate::php_version::PhpVersion;
use crate::types::Type;
use syntax::ast::Arg;

/// Directives removed from PHP, with the version that removed them.
const REMOVED: &[(&str, PhpVersion)] = &[
    ("sql.safe_mode", (7, 2)),
    ("opcache.fast_shutdown", (7, 2)),
    ("birdstep.max_links", (7, 3)),
    ("opcache.inherited_hack", (7, 3)),
    ("track_errors", (8, 0)),
    ("mbstring.func_overload", (8, 0)),
    ("pdo_odbc.db2_instance_name", (8, 0)),
    ("assert.quiet_eval", (8, 0)),
    ("mysqlnd.fetch_data_copy", (8, 1)),
    ("log_errors_max_len", (8, 1)),
    ("mysqli.reconnect", (8, 2)),
    ("opcache.consistency_checks", (8, 3)),
    ("disable_classes", (8, 5)),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Deprecation {
    /// Any use of the directive.
    Directive,
    /// Only changing its value.
    Changing,
}

/// Deprecated directives, with the version that deprecated them.
const DEPRECATED: &[(&str, PhpVersion, Deprecation)] = &[
    ("track_errors", (7, 2), Deprecation::Directive),
    ("mbstring.func_overload", (7, 2), Deprecation::Directive),
    ("pdo_odbc.db2_instance_name", (7, 3), Deprecation::Directive),
    ("allow_url_include", (7, 4), Deprecation::Directive),
    ("filter.default", (8, 1), Deprecation::Directive),
    (
        "oci8.old_oci_close_semantics",
        (8, 1),
        Deprecation::Directive,
    ),
    ("auto_detect_line_endings", (8, 1), Deprecation::Directive),
    ("assert.active", (8, 3), Deprecation::Changing),
    ("assert.bail", (8, 3), Deprecation::Changing),
    ("assert.callback", (8, 3), Deprecation::Changing),
    ("assert.exception", (8, 3), Deprecation::Changing),
    ("assert.warning", (8, 3), Deprecation::Changing),
    ("session.sid_length", (8, 4), Deprecation::Changing),
    (
        "session.sid_bits_per_character",
        (8, 4),
        Deprecation::Changing,
    ),
    ("session.use_only_cookies", (8, 4), Deprecation::Changing),
    ("session.use_trans_sid", (8, 4), Deprecation::Changing),
    ("session.trans_sid_tags", (8, 4), Deprecation::Changing),
    ("session.trans_sid_hosts", (8, 4), Deprecation::Changing),
    ("session.referer_check", (8, 4), Deprecation::Changing),
    ("report_memleaks", (8, 5), Deprecation::Directive),
    ("intl.error_level", (8, 5), Deprecation::Directive),
];

fn directive_names(ty: &Type) -> Vec<&str> {
    match ty {
        Type::StringLiteral(s) => vec![s.as_str()],
        Type::Union(members) => members.iter().flat_map(directive_names).collect(),
        _ => Vec::new(),
    }
}

impl<'a> Checker<'a> {
    /// `function` is the lowercase name of an ini-accessing builtin.
    pub(crate) fn check_ini_directive_call(
        &mut self,
        function: &str,
        args: &[Arg],
        locals: &Locals,
        label: &str,
    ) {
        let Some(version) = self.registry.php_version else {
            return;
        };
        let Some(arg) = args
            .first()
            .filter(|a| !a.spread && a.name.is_none())
            .or_else(|| {
                args.iter().find(|a| {
                    a.name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case("option"))
                })
            })
        else {
            return;
        };
        let changes = matches!(function, "ini_set" | "ini_alter");
        let ty = self.infer_expr(&arg.value, locals);
        for directive in directive_names(&ty) {
            if let Some((_, removed)) = REMOVED
                .iter()
                .find(|(name, removed)| *name == directive && version >= *removed)
            {
                let outcome = if function == "ini_restore" {
                    "it does nothing"
                } else {
                    "it always returns false"
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Call to {function}() with ini directive {directive}, removed in PHP {}.{}; {outcome}.",
                        removed.0, removed.1
                    ),
                    arg.value.span,
                    0,
                    "ini.removed",
                );
                continue;
            }
            if !self.registry.check_deprecated_usage || self.current_scope_deprecated {
                continue;
            }
            if let Some((_, since, kind)) = DEPRECATED.iter().find(|(name, since, kind)| {
                *name == directive
                    && version >= *since
                    && (changes || *kind == Deprecation::Directive)
            }) {
                let what = if *kind == Deprecation::Changing {
                    "Changing ini directive"
                } else {
                    "Ini directive"
                };
                self.report(
                    label.to_string(),
                    format!(
                        "Call to {function}(): {what} {directive} is deprecated since PHP {}.{}.",
                        since.0, since.1
                    ),
                    arg.value.span,
                    0,
                    "ini.deprecated",
                );
            }
        }
    }
}
