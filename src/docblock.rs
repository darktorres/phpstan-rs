use std::collections::{HashMap, HashSet};

/// A parsed subset of a `/** ... */` doc comment: just the tags this
/// prototype's type checker can use (`@param`, `@return`, `@var`). Types are
/// kept as raw strings here, resolved through `types::parse_type_string`
/// only where a caller actually needs a `Type` - this module doesn't depend
/// on `types` at all, so it can't unintentionally do (or fail to do) any
/// resolution work of its own.
///
/// Deliberately does not parse conditional return types - those need real
/// per-call-site type parameter binding, not just parsing. `@param`/
/// `@return`/`@var` cover PHPDoc's single biggest practical win with the
/// least machinery: adding a type to something a native hint can't express
/// (an untyped param/property, a native `array` refined to a shape or
/// generic, or PHP versions without union types).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocBlock {
    pub params: HashMap<String, String>,
    pub return_type: Option<String>,
    pub var_type: Option<String>,
    // The `$name` half of `@var Type $name`, when present - the common
    // property-level `@var Type` (no name) form leaves this `None`; an
    // inline `@var` on an arbitrary statement (checker::stmt) uses this to
    // know which variable it names, when the bare form's own "assumed from
    // the statement's own single assignment target" heuristic doesn't
    // apply (e.g. a comment naming a variable other than the one that
    // immediately follows it).
    pub var_name: Option<String>,
    // `@template T` names, in declaration order (order matters for
    // positional binding - see `registry::ClassInfo::templates`).
    pub templates: Vec<String>,
    // `@template T of Bound` - the bound text for whichever of `templates`
    // above declared one (Phase 10.5's bound-enforcement half). A `T` with
    // no `of` clause simply has no entry here.
    pub template_bounds: HashMap<String, String>,
    // `@template T = Default` (Phase 54) - the default-type text for
    // whichever of `templates` above declared one, used when an "erased"
    // reference (fewer type arguments than declared, including a bare
    // one with none at all) leaves this name unbound instead of falling
    // back to `Mixed` unconditionally. A `T` with no `=` clause simply
    // has no entry here, same shape as `template_bounds` just above.
    pub template_defaults: HashMap<String, String>,
    // Names from `templates` above additionally declared
    // `@template-covariant`/`@template-contravariant` (Phase 10.5's
    // variance half) - a name absent from both stays invariant, this
    // project's only behavior before Phase 10.5 and still the default.
    pub covariant_templates: HashSet<String>,
    pub contravariant_templates: HashSet<String>,
    // Names of `@param`s tagged `[optional]` - a real, longstanding PHP
    // manual/phpstorm-stubs convention marking a native parameter as
    // conceptually optional even when its own signature has no `= value`
    // default (phpstorm-stubs models several functions, e.g. `mt_rand`'s
    // `$max` and `array_keys`'s `$filter_value`, only as their
    // highest-arity overload this way).
    pub optional_params: HashSet<String>,
    // `@extends Parent<Arg, ...>` - fixes (some of) the parent class's own
    // `@template` names for every instance of this class, regardless of
    // whether this class is itself generic (Phase 10.5). At most one is
    // meaningful (a class has a single native `extends`), so the last one
    // seen wins, matching every other single-valued tag here.
    pub extends: Option<String>,
    // `@implements Iface<Arg, ...>` - same idea as `extends`, but a class
    // can `implements` more than one interface at once, so every tag is
    // kept (each one names which interface it applies to via its own
    // leading type name).
    pub implements: Vec<String>,
    // `@property`/`@property-read`/`@property-write Type $name` (Phase
    // 13 Finding D) - magic properties, typically backed by `__get`/
    // `__set`, declared on a class's own docblock rather than a real
    // field. A class can declare many, so every tag is kept.
    pub properties: Vec<PropertyTag>,
    // Phase 40: real PHPStan's own backward-compatibility-promise marker
    // - a class/interface/trait tagged `@api` is safe for a third-party
    // extension author to reference; anything else may change in a minor
    // version with no notice. Consumed by `checker::phpstan_api`, gated
    // behind the analyzed project actually depending on `phpstan/phpstan`
    // (see `composer::detect_phpstan_dependency`).
    pub api: bool,
    // `@api-do-not-implement` (always found paired with a plain `@api` on
    // the same docblock in real PHPStan's own source, confirmed directly
    // against a fresh `phpstan/phpstan-src` clone - e.g.
    // `Analyser/Scope.php`, `Reflection/ReflectionProvider.php`) - an
    // `@api` interface/class additionally marked this way may gain new
    // abstract members in a minor version, so third-party code must not
    // `implements`/`extends` it, even though calling its existing methods
    // is still fully covered. Implies `api` on its own (see `apply_tag`'s
    // "api-do-not-implement" arm) so a docblock that only ever writes the
    // longer tag alone still counts as covered for a plain reference.
    pub api_do_not_implement: bool,
    // Phase 67: `@internal` - marks a class/interface/trait/enum,
    // function, method, or property as usable only from within its own
    // declaring "root" (top-level) namespace, the common Composer-
    // package-as-top-level-namespace convention. Consumed by
    // `checker::internal_usage`, gated behind
    // `Registry::check_internal_usage` (default off, matching real
    // PHPStan's own `featureToggles.internalTag: false` - this rule
    // family isn't part of any of the standard level 0-10 rule sets
    // either, same non-default posture Phase 62's own
    // `check_uninitialized_properties` already established a precedent
    // for).
    pub internal: bool,
    // Phase 39: `@deprecated` on a class/interface/trait/enum, function,
    // method, property, or class constant - `None` when the tag isn't
    // present at all; `Some(message)` when it is, where `message` is
    // everything after the tag name, trimmed (`Some(String::new())` for
    // a bare `@deprecated` with no reason given, a real, common shape -
    // confirmed directly: 491 real files in this project's own Piwigo+
    // vendor verification corpus use the tag at all, many with no
    // message). Note this is the PHPDoc *tag* specifically, a purely
    // documentation/static-analysis convention with zero PHP runtime
    // effect of its own - distinct from PHP 8.4's native `#[\Deprecated]`
    // *attribute* (which does emit a real runtime `E_DEPRECATED`
    // warning on use, confirmed directly via `php`), not handled here.
    // Consumed by `checker::deprecated_usage`, gated behind `Registry::
    // check_deprecated_usage` (default off, mirroring `check_internal_
    // usage`'s own identical "off by default, real value but real vendor
    // noise" precedent - real PHPStan's own equivalent isn't even part
    // of core `phpstan/phpstan` at all, but a separate, opt-in Composer
    // package, `phpstan/phpstan-deprecation-rules`).
    pub deprecated: Option<String>,
    // Phase 42's own self-contained first slice: `@throws Type[,
    // description]` on a function/method - the checked-exception type a
    // caller should be prepared for. A function can (and often does)
    // document more than one, one per line, so every tag is kept (same
    // "a class can declare many" reasoning `implements`/`properties`
    // above already have) - order isn't meaningful, only "which types
    // were named at all," consumed as a union. Only the raw type text is
    // kept here, same as `return_type`/`var_type` above - resolved to a
    // real `Type` (and validated as an actual `Throwable` subtype) at
    // registry-build time, where name resolution/generics substitution
    // is actually available.
    pub throws: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTag {
    pub name: String,
    pub ty: String,
    pub readable: bool,
    pub writable: bool,
}

pub fn parse(raw: &str) -> DocBlock {
    let mut block = DocBlock::default();

    let inner = raw.trim().trim_start_matches("/**").trim_end_matches("*/");

    // A tag's own text can legitimately wrap onto following `*`-prefixed
    // lines - most commonly a long array-shape type too wide for one
    // line (`@return array{id: int, name: string,\n *   ...}`, a real,
    // common style for documenting large DTOs). Every line joins onto
    // the most recent `@tag` line until the next one starts, rather than
    // being parsed as its own (tagless, ignored) line - take_type_token
    // below still correctly stops at the type's own closing bracket
    // followed by whitespace, discarding any joined-on description text
    // that follows it on the same logical line.
    let mut tag_lines: Vec<String> = Vec::new();
    for raw_line in inner.lines() {
        let line = raw_line.trim();
        let line = line.strip_prefix('*').map(str::trim).unwrap_or(line);
        if line.starts_with('@') || tag_lines.is_empty() {
            tag_lines.push(line.to_string());
        } else if let Some(last) = tag_lines.last_mut() {
            last.push(' ');
            last.push_str(line);
        }
    }

    // Real PHPStan recognizes a `@phpstan-`-prefixed sibling of `@param`/
    // `@return`/`@var`/`@template*`/`@extends`/`@implements` as a more
    // precise, PHPStan-only override of the same information - a common
    // convention (582/437/186 occurrences of `@phpstan-param`/`-return`/
    // `-var` alone across this project's own vendored corpus, mostly
    // Doctrine's own stubs, e.g. `EntityRepository::findBy()`'s native
    // `@return` staying the loose `array` an IDE/other tools understand
    // while `@phpstan-return list<T>` carries the precise generic type)
    // meant to coexist with a plainer tag for non-PHPStan tooling, not
    // replace it outright. Two passes over the same tag list - plain tags
    // first, then `@phpstan-*` ones with the prefix stripped, reusing the
    // exact same per-tag logic - gives the override its right-to-win
    // regardless of which one the docblock happens to write first.
    for line in &tag_lines {
        let Some(tag_body) = line.strip_prefix('@') else {
            continue;
        };
        if tag_body.starts_with("phpstan-") {
            continue;
        }
        let mut parts = tag_body.splitn(2, char::is_whitespace);
        let tag = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        apply_tag(&mut block, tag, rest);
    }
    for line in &tag_lines {
        let Some(tag_body) = line.strip_prefix('@') else {
            continue;
        };
        let Some(tag_body) = tag_body.strip_prefix("phpstan-") else {
            continue;
        };
        let mut parts = tag_body.splitn(2, char::is_whitespace);
        let tag = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        apply_tag(&mut block, tag, rest);
    }

    block
}

fn apply_tag(block: &mut DocBlock, tag: &str, rest: &str) {
    {
        match tag {
            "param" => {
                // Convention is `Type $name description...`.
                let ty = take_type_token(rest);
                let remainder = rest[ty.len()..].trim_start();
                let var_token = remainder.split_whitespace().next().unwrap_or("");
                // A by-ref param is conventionally documented as `&$name`
                // (real, common PHP-manual style, e.g. phpstorm-stubs'
                // `@param string[] &$matches [optional]`) - strip the
                // leading `&` too, or the name never matches at all and
                // both its doc type and `[optional]` marker are silently
                // dropped. Same idea for a documented variadic parameter
                // (`@param int ...$nums`) - without stripping the leading
                // `...`, `strip_prefix('$')` fails on the literal `.`,
                // silently losing the type for a real, common PHPDoc
                // convention (confirmed present throughout the vendored
                // phpstorm-stubs corpus itself, e.g. `gd`/`xdebug`/
                // `RedisCluster`).
                if let Some(name) = var_token
                    .trim_start_matches('&')
                    .trim_start_matches("...")
                    .strip_prefix('$')
                {
                    if !ty.is_empty() {
                        block.params.insert(name.to_string(), ty.to_string());
                    }
                    let after_name = remainder[var_token.len()..].trim_start();
                    if after_name.starts_with("[optional]") {
                        block.optional_params.insert(name.to_string());
                    }
                }
            }
            "return" => {
                let ty = take_type_token(rest);
                // This module's own doc comment already disclaims
                // conditional return types (`($param is Foo ? Bar : Baz)`,
                // real PHPStan syntax Doctrine DBAL's own
                // `DriverManager::getConnection()` uses) as needing real
                // per-call-site binding this project doesn't do - but
                // before `@phpstan-return` support existed, no real
                // `@return` in this project's corpus ever actually used
                // one, so nothing enforced that disclaimer. `take_type_token`
                // has no notion of the `is`/`?`/`:` conditional-type
                // grammar; it just grabs the whole balanced-parens token,
                // which `types::parse_type_string` then can't meaningfully
                // parse either - leaving `return_type` unset (rather than
                // a garbage type) is what "doesn't parse" actually has to
                // mean once a real conditional expression reaches here.
                if !ty.is_empty() && !ty.starts_with("($") {
                    block.return_type = Some(ty.to_string());
                }
            }
            "var" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.var_type = Some(ty.to_string());
                    let remainder = rest[ty.len()..].trim_start();
                    if let Some(name) = remainder
                        .split_whitespace()
                        .next()
                        .and_then(|t| t.strip_prefix('$'))
                    {
                        block.var_name = Some(name.to_string());
                    }
                }
            }
            "property" | "property-read" | "property-write" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    let remainder = rest[ty.len()..].trim_start();
                    if let Some(name) = remainder
                        .split_whitespace()
                        .next()
                        .and_then(|t| t.strip_prefix('$'))
                    {
                        block.properties.push(PropertyTag {
                            name: name.to_string(),
                            ty: ty.to_string(),
                            readable: tag != "property-write",
                            writable: tag != "property-read",
                        });
                    }
                }
            }
            "template" => {
                if let Some((name, bound, default)) = parse_template_tag(rest) {
                    if !block.templates.contains(&name) {
                        block.templates.push(name.clone());
                    }
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name.clone(), bound);
                    }
                    if let Some(default) = default {
                        block.template_defaults.insert(name, default);
                    }
                }
            }
            // `@template-covariant`/`@template-contravariant` (Phase
            // 10.5's variance half) - same shape as a plain `@template`,
            // just also marking which direction the parameter is allowed
            // to vary in.
            "template-covariant" => {
                if let Some((name, bound, default)) = parse_template_tag(rest) {
                    if !block.templates.contains(&name) {
                        block.templates.push(name.clone());
                    }
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name.clone(), bound);
                    }
                    if let Some(default) = default {
                        block.template_defaults.insert(name.clone(), default);
                    }
                    block.covariant_templates.insert(name);
                }
            }
            "template-contravariant" => {
                if let Some((name, bound, default)) = parse_template_tag(rest) {
                    if !block.templates.contains(&name) {
                        block.templates.push(name.clone());
                    }
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name.clone(), bound);
                    }
                    if let Some(default) = default {
                        block.template_defaults.insert(name.clone(), default);
                    }
                    block.contravariant_templates.insert(name);
                }
            }
            "extends" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.extends = Some(ty.to_string());
                }
            }
            "implements" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.implements.push(ty.to_string());
                }
            }
            // Phase 40: see `DocBlock::api`/`api_do_not_implement`'s own
            // doc comments. Neither tag takes an argument, so `rest` is
            // never consulted - unlike every other arm above.
            "api" => {
                block.api = true;
            }
            "api-do-not-implement" => {
                block.api = true;
                block.api_do_not_implement = true;
            }
            // Phase 67: see `DocBlock::internal`'s own doc comment. Same
            // no-argument shape as `@api` just above.
            "internal" => {
                block.internal = true;
            }
            // Phase 39: see `DocBlock::deprecated`'s own doc comment.
            // Unlike `@api`/`@internal`, this tag *does* usually take a
            // free-text argument (the deprecation reason) - kept
            // verbatim, trimmed, rather than discarded the way a no-
            // argument tag's own `rest` is.
            "deprecated" => {
                block.deprecated = Some(rest.trim().to_string());
            }
            "throws" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.throws.push(ty.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Parses `@template`'s own body: `T` alone, `T of Bound`, `T = Default`
/// (Phase 54), or `T of Bound = Default` with both together, in that
/// order - real PHPStan's own grammar always writes the bound (when
/// present) before the default. Either a bound or a default can itself be
/// a union (`T of Foo|Bar`, `T = Foo|Bar`), so both go through
/// `take_type_token` like every other type token here, not a bare
/// `split_whitespace`. Returns `None` only when there's no name at all (a
/// malformed/empty tag).
fn parse_template_tag(rest: &str) -> Option<(String, Option<String>, Option<String>)> {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next()?.to_string();
    if name.is_empty() {
        return None;
    }
    let mut remainder = parts.next().unwrap_or("").trim();
    let bound = match remainder
        .strip_prefix("of")
        .filter(|s| s.starts_with(char::is_whitespace))
    {
        Some(s) => {
            let s = s.trim_start();
            let bound = take_type_token(s);
            remainder = s[bound.len()..].trim_start();
            (!bound.is_empty()).then(|| bound.to_string())
        }
        None => None,
    };
    let default = remainder.strip_prefix('=').and_then(|s| {
        let default = take_type_token(s.trim_start());
        (!default.is_empty()).then(|| default.to_string())
    });
    Some((name, bound, default))
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
mod tests;
