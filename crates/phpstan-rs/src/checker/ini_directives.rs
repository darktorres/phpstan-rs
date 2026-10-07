//! `ini_get()`/`ini_set()`/`ini_alter()`/`ini_restore()`/`get_cfg_var()`
//! naming a directive the configured PHP version no longer has
//! (`ini.removed`: the call returns `false` and changes nothing) or has
//! deprecated (`ini.deprecated`, with `check_deprecated_usage`). Some
//! directives are deprecated only to change, so reading them is fine.

use crate::checker::{Checker, Locals};
use syntax::ast::{Arg, NodeArenas};
use types::PhpVersion;

/// Directives removed from PHP, with the version that removed them.
const REMOVED: &[(&str, PhpVersion)] = &[
    ("sql.safe_mode", PhpVersion::new(7, 2)),
    ("opcache.fast_shutdown", PhpVersion::new(7, 2)),
    ("birdstep.max_links", PhpVersion::new(7, 3)),
    ("opcache.inherited_hack", PhpVersion::new(7, 3)),
    ("track_errors", PhpVersion::new(8, 0)),
    ("mbstring.func_overload", PhpVersion::new(8, 0)),
    ("pdo_odbc.db2_instance_name", PhpVersion::new(8, 0)),
    ("assert.quiet_eval", PhpVersion::new(8, 0)),
    ("mysqlnd.fetch_data_copy", PhpVersion::new(8, 1)),
    ("log_errors_max_len", PhpVersion::new(8, 1)),
    ("mysqli.reconnect", PhpVersion::new(8, 2)),
    ("opcache.consistency_checks", PhpVersion::new(8, 3)),
    ("disable_classes", PhpVersion::new(8, 5)),
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
    (
        "track_errors",
        PhpVersion::new(7, 2),
        Deprecation::Directive,
    ),
    (
        "mbstring.func_overload",
        PhpVersion::new(7, 2),
        Deprecation::Directive,
    ),
    (
        "pdo_odbc.db2_instance_name",
        PhpVersion::new(7, 3),
        Deprecation::Directive,
    ),
    (
        "allow_url_include",
        PhpVersion::new(7, 4),
        Deprecation::Directive,
    ),
    (
        "filter.default",
        PhpVersion::new(8, 1),
        Deprecation::Directive,
    ),
    (
        "oci8.old_oci_close_semantics",
        PhpVersion::new(8, 1),
        Deprecation::Directive,
    ),
    (
        "auto_detect_line_endings",
        PhpVersion::new(8, 1),
        Deprecation::Directive,
    ),
    (
        "assert.active",
        PhpVersion::new(8, 3),
        Deprecation::Changing,
    ),
    ("assert.bail", PhpVersion::new(8, 3), Deprecation::Changing),
    (
        "assert.callback",
        PhpVersion::new(8, 3),
        Deprecation::Changing,
    ),
    (
        "assert.exception",
        PhpVersion::new(8, 3),
        Deprecation::Changing,
    ),
    (
        "assert.warning",
        PhpVersion::new(8, 3),
        Deprecation::Changing,
    ),
    (
        "session.sid_length",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.sid_bits_per_character",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.use_only_cookies",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.use_trans_sid",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.trans_sid_tags",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.trans_sid_hosts",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "session.referer_check",
        PhpVersion::new(8, 4),
        Deprecation::Changing,
    ),
    (
        "report_memleaks",
        PhpVersion::new(8, 5),
        Deprecation::Directive,
    ),
    (
        "intl.error_level",
        PhpVersion::new(8, 5),
        Deprecation::Directive,
    ),
];

impl<'a> Checker<'a> {
    /// `function` is the lowercase name of an ini-accessing builtin.
    pub(crate) fn check_ini_directive_call(
        &mut self,
        arenas: &NodeArenas,
        function: &str,
        args: &[Arg],
        locals: &Locals,
    ) {
        let Some(version) = self.registry.php_version_max() else {
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
        let Some(value) = arenas.expr(arg.value) else {
            return;
        };
        let ty = self.infer_expr(value, locals);
        for directive in ty.constant_strings_text() {
            if let Some((_, removed)) = REMOVED
                .iter()
                .find(|(name, removed)| *name == directive && version >= *removed)
            {
                let outcome = if function == "ini_restore" {
                    "it does nothing"
                } else {
                    "it always returns false"
                };
                self.report(format!(
                        "Call to {function}() with ini directive {directive}, removed in PHP {removed}; {outcome}."
                    ),
                    self.expr_span(value.id),
                    0,
                    "ini.removed",
                );
                continue;
            }
            if !self.settings.deprecation_rules || self.current_scope_deprecated {
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
                    format!(
                        "Call to {function}(): {what} {directive} is deprecated since PHP {since}."
                    ),
                    self.expr_span(value.id),
                    0,
                    "ini.deprecated",
                );
            }
        }
    }
}
