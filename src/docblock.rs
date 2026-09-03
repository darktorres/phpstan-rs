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

    for line in &tag_lines {
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
                if !ty.is_empty() {
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
                if let Some((name, bound)) = parse_template_tag(rest) {
                    block.templates.push(name.clone());
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name, bound);
                    }
                }
            }
            // `@template-covariant`/`@template-contravariant` (Phase
            // 10.5's variance half) - same shape as a plain `@template`,
            // just also marking which direction the parameter is allowed
            // to vary in.
            "template-covariant" => {
                if let Some((name, bound)) = parse_template_tag(rest) {
                    block.templates.push(name.clone());
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name.clone(), bound);
                    }
                    block.covariant_templates.insert(name);
                }
            }
            "template-contravariant" => {
                if let Some((name, bound)) = parse_template_tag(rest) {
                    block.templates.push(name.clone());
                    if let Some(bound) = bound {
                        block.template_bounds.insert(name.clone(), bound);
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
            _ => {}
        }
    }

    block
}

/// Parses `@template`'s own body: `T` alone, or `T of Bound` (a bound can
/// itself be a union, e.g. `T of Foo|Bar`, so it goes through
/// `take_type_token` like every other type token here, not a bare
/// `split_whitespace`). Returns `None` only when there's no name at all
/// (a malformed/empty tag).
fn parse_template_tag(rest: &str) -> Option<(String, Option<String>)> {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next()?.to_string();
    if name.is_empty() {
        return None;
    }
    let after_name = parts.next().unwrap_or("").trim();
    let bound = after_name
        .strip_prefix("of")
        .filter(|s| s.starts_with(char::is_whitespace))
        .map(|s| take_type_token(s.trim_start()))
        .filter(|b| !b.is_empty())
        .map(str::to_string);
    Some((name, bound))
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
