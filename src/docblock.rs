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
    // `@phpstan-consistent-constructor` - a class-level bare marker (no
    // value, same shape as `@api`/`@internal` above): every subclass's
    // own `__construct` must match this class's own exactly (arity,
    // types, visibility - the identical compatibility rules an ordinary
    // method override already gets, deliberately exempted for `__construct`
    // otherwise, real PHP's own default). Consumed by `Registry::
    // find_consistent_constructor_ancestor`/`checker::oop::check_one_
    // method_override`'s own `__construct` special case.
    pub consistent_constructor: bool,
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
    // `@phpstan-assert[-if-true/-if-false] [!]Type $target` - real
    // PHPStan's custom-assertion-function convention (`assertNarrowedTo
    // Type`-style helpers, real corpus shape: PHPUnit's own accessor
    // pair `hasTestFilesFile(): bool` tagged `@phpstan-assert-if-true
    // !null $this->testFilesFile`, then `testFilesFile(): string`'s own
    // body guards on it). `assert` is unconditional (true regardless of
    // this declaration's own return value - the common shape for a
    // function that throws on failure, mirroring `assert($cond)`'s own
    // existing "narrows for the rest of the scope, no branch needed"
    // treatment); `assert_if_true`/`assert_if_false` only hold once this
    // declaration's own return value is known. `target`/`ty` are kept as
    // raw text here, same "parse now, resolve later" split every other
    // type-bearing tag above already has - resolved into a real `Type`
    // plus a classified `registry::AssertTarget` at registry-build time.
    // A real, common target shape this doesn't attempt: `=Type`/`!=value`
    // (PHPStan's own template-equality/value-equality assertion forms,
    // a genuinely different check than "is this a subtype of Type" -
    // `parse_assert_tag` below skips these entirely rather than
    // misparsing them into a bogus type assertion).
    pub assert_if_true: Vec<AssertTag>,
    pub assert_if_false: Vec<AssertTag>,
    pub assert: Vec<AssertTag>,
    // `@phpstan-type Name TypeExpr` - a local type alias, conventionally
    // declared on a class's own docblock and referenced bare (by `Name`
    // alone) from that same class's `@param`/`@return`/`@var` tags (real
    // corpus shape: `justinrainbow/json-schema`'s own `ErrorBag`,
    // `@phpstan-type Error array{...}` then `@phpstan-type ErrorList
    // list<Error>`). Kept as raw text here, same "parse now, resolve
    // later" split every other type-bearing tag has - resolved into a
    // real `Type` (and substituted into every member that references it)
    // at `ClassInfo::from_decl` time, where name resolution is actually
    // available.
    pub type_aliases: HashMap<String, String>,
    // `@phpstan-import-type Name from Class [as Alias]` - the cross-class
    // sibling of `@phpstan-type` above, importing another class's own
    // local alias by name instead of declaring a fresh one. Kept as raw
    // text here too (`from`/`alias` are simple bare identifiers per real
    // `phpstan/phpdoc-parser`'s own grammar, never subject to `use`-import
    // rewriting the way a type expression's own class references are) -
    // resolved once `Registry::resolve_deferred_type_imports` can see
    // every class's own already-built `type_aliases` table, which a
    // single class's own `from_decl` call can't (the source class may not
    // be indexed yet at all).
    pub type_imports: Vec<TypeImportTag>,
    // `@phpstan-require-extends Type` - declared on a trait/interface,
    // requiring every class that ends up using/implementing it to also
    // `extends` (or otherwise be a subtype of) `Type`. Kept as raw text,
    // resolved into a real `Type` at `ClassInfo::from_decl` time (same
    // "parse now, resolve later" split as every other type-bearing tag) -
    // `checker::oop::check_require_tags` walks every real consumer.
    pub require_extends: Vec<String>,
    // `@phpstan-require-implements Type` - a trait's own sibling of
    // `require_extends` above, requiring the using class to `implements`
    // `Type` instead (real PHPStan doesn't recognize this on an interface
    // at all - an interface can't `implements`, only `extends`).
    pub require_implements: Vec<String>,
    // `@phpstan-sealed Type` - declares the *complete* list of types
    // allowed to directly `extends`/`implements` this one (a union of
    // several is real, common syntax for "exactly these"). Unlike
    // `require_extends`/`require_implements` above (a transitive "must be
    // a subtype of," checked against every real consumer anywhere in the
    // hierarchy), this is a one-level, exact-name allowlist checked
    // against every *immediate* child only - `checker::oop::
    // check_sealed_hierarchy`.
    pub sealed: Vec<String>,
    // `@method [static] ReturnType methodName(Type $param = default, ...)` -
    // a virtual method with no real declaration at all (conventionally
    // backed by `__call`/`__callStatic`). A class can declare many, one
    // tag per virtual method. Kept parsed (not raw text like most other
    // tags here) since the grammar is genuinely more than one type
    // expression - `parse_method_tag`'s own doc comment has the details.
    pub method_tags: Vec<MethodTag>,
    // `@mixin Type` - every `Type`'s own *public* method/property becomes
    // available on this class too, as if inherited (conventionally
    // forwarded via `__call`/`__get`). Kept as raw text, resolved at
    // `Registry::resolve_deferred_mixins` time (the mixin target may not
    // be indexed yet at `ClassInfo::from_decl` time, the identical
    // ordering problem `type_imports` already has).
    pub mixins: Vec<String>,
}

/// One parsed `@phpstan-import-type` tag - see `DocBlock::type_imports`'s
/// own doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeImportTag {
    // The name as declared on the *source* class's own `@phpstan-type`.
    pub alias: String,
    // The source class, raw text (resolved against *this* class's own
    // `use`-imports/namespace at `ClassInfo::from_decl` time, same as
    // `extends`/`implements`).
    pub from: String,
    // `as OtherName` - the name this class's own members reference it by,
    // when different from `alias`.
    pub as_name: Option<String>,
}

/// One parsed `@method` tag - see `DocBlock::method_tags`'s own doc
/// comment.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodTag {
    pub is_static: bool,
    // `None` when the tag omits a return type entirely (real, legal
    // syntax - `@method getX()` - resolved to `Mixed` at `ClassInfo::
    // from_decl` time, same as an omitted native return type anywhere
    // else in this project).
    pub return_type: Option<String>,
    pub name: String,
    pub params: Vec<MethodTagParam>,
}

/// One parameter of a parsed `@method` tag.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodTagParam {
    // `None` when the parameter's own type is omitted (also real, legal
    // syntax) - resolved to `Mixed`, same as `MethodTag::return_type`.
    pub ty: Option<String>,
    pub name: String,
    pub by_ref: bool,
    pub variadic: bool,
    pub has_default: bool,
}

/// One parsed `@phpstan-assert`-family tag - see `DocBlock::assert_if_true`'s
/// own doc comment for the full picture. `target` is kept as raw text
/// (`"$actual"`, `"$this"`, `"$this->prop"`) - classified into a real
/// `registry::AssertTarget` once a declaration's own parameter list is
/// available to match a bare name against.
#[derive(Debug, Clone, PartialEq)]
pub struct AssertTag {
    pub negated: bool,
    pub ty: String,
    pub target: String,
}

/// Parses one `@phpstan-assert`-family tag's own `rest` text
/// (`"!null $this->testFilesFile"`) into a negation flag, a type text,
/// and a target text - `None` for the `=Type`/`!=value` forms this
/// project doesn't model (a genuinely different, value/generic-argument
/// comparison, not a type check `take_type_token` could parse
/// meaningfully anyway).
fn parse_assert_tag(rest: &str) -> Option<AssertTag> {
    let rest = rest.trim();
    if rest.starts_with("!=") || rest.starts_with('=') {
        return None;
    }
    let (negated, rest) = match rest.strip_prefix('!') {
        Some(after) => (true, after),
        None => (false, rest),
    };
    let ty = take_type_token(rest);
    if ty.is_empty() {
        return None;
    }
    let target = rest[ty.len()..].trim();
    if target.is_empty() {
        return None;
    }
    Some(AssertTag {
        negated,
        ty: ty.to_string(),
        target: target.to_string(),
    })
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
        // A wrapped array-shape/list field commonly carries its own
        // trailing `// description` (real corpus shape, e.g. pestphp's
        // `JavaScriptSerializer::parseValue`'s `@var array{v?: string, //
        // null, undefined, ..., b?: bool, // boolean, ...}`). Without
        // stripping it, the comment text gets spliced straight into the
        // joined type string below and `take_type_token` happily parses
        // "// null" as a bare class-like name, corrupting the whole
        // shape - `//` never appears in real type-expression syntax, so
        // stripping from its first occurrence to end of line is safe.
        let line = line
            .split_once("//")
            .map_or(line, |(before, _)| before.trim());
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
            "type" => {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("");
                let remainder = parts.next().unwrap_or("").trim_start();
                // A real, common variant of this tag's own syntax writes
                // an `=` between the alias name and its type expression
                // (real corpus shape: `doctrine/orm`'s own `@phpstan-type
                // ConcreteAssociationMapping = OneToOneOwningSideMapping|
                // ...`) - without stripping it, `take_type_token` stopped
                // at the first whitespace and took the bare `=` itself as
                // the whole "type", later resolved into a bogus namespace-
                // qualified class name ending in `\=`.
                let remainder = remainder
                    .strip_prefix('=')
                    .unwrap_or(remainder)
                    .trim_start();
                let ty = take_type_token(remainder);
                if !name.is_empty() && !ty.is_empty() {
                    block.type_aliases.insert(name.to_string(), ty.to_string());
                }
            }
            "import-type" => {
                // `Alias from ClassName [as OtherAlias]` - a fixed
                // keyword grammar (confirmed directly against real
                // `phpstan/phpdoc-parser`'s own `parseTypeAliasImportTagValue`),
                // not a type expression, so plain whitespace-splitting is
                // enough - no `take_type_token` involved.
                let mut parts = rest.split_whitespace();
                let alias = parts.next().unwrap_or("");
                let from = if parts.next() == Some("from") {
                    parts.next().unwrap_or("")
                } else {
                    ""
                };
                if !alias.is_empty() && !from.is_empty() {
                    let as_name = if parts.next() == Some("as") {
                        parts.next().map(str::to_string)
                    } else {
                        None
                    };
                    block.type_imports.push(TypeImportTag {
                        alias: alias.to_string(),
                        from: from.to_string(),
                        as_name,
                    });
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
            "require-extends" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.require_extends.push(ty.to_string());
                }
            }
            "require-implements" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.require_implements.push(ty.to_string());
                }
            }
            "sealed" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.sealed.push(ty.to_string());
                }
            }
            "method" => {
                if let Some(tag) = parse_method_tag(rest) {
                    block.method_tags.push(tag);
                }
            }
            "mixin" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.mixins.push(ty.to_string());
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
            "consistent-constructor" => {
                block.consistent_constructor = true;
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
            "assert-if-true" => {
                if let Some(tag) = parse_assert_tag(rest) {
                    block.assert_if_true.push(tag);
                }
            }
            "assert-if-false" => {
                if let Some(tag) = parse_assert_tag(rest) {
                    block.assert_if_false.push(tag);
                }
            }
            "assert" => {
                if let Some(tag) = parse_assert_tag(rest) {
                    block.assert.push(tag);
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
/// off at its first internal space (`array{count: int, ...}`). A `'...'`/
/// `"..."` string-literal segment (a real, legal PHPStan literal-string-
/// union member, e.g. `' '|'>'|'+'|'~'`) is tracked as one opaque run
/// too - without this, a space *inside* the quotes (a real corpus shape:
/// `sabberworm/php-css-parser`'s own `@phpstan-type ValidCombinatorValue
/// ' '|'>'|'+'|'~'`) was wrongly treated as the token's own end, cutting
/// the whole type down to a bare, dangling `'`.
fn take_type_token(s: &str) -> &str {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => return &s[..i],
            _ => {}
        }
    }
    s
}

/// Splits `s` on `,`, but only where it appears outside any `<>`/`{}`/
/// `()`/`[]` nesting - `@method`'s own parameter list needs this (a real
/// corpus shape: `array<string, mixed> $options = []`, whose own internal
/// comma must not split the parameter list itself). Mirrors `take_type_
/// token`'s exact bracket-depth/quote-tracking shape one call up - a
/// deliberate duplication, not shared with `types::split_top_level`
/// (nearly identical logic, comma instead of whitespace as the
/// separator): this module's own doc comment already commits to zero
/// dependency on `types` at all.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '<' | '{' | '(' | '[' => depth += 1,
            '>' | '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// `@method [static] ReturnType methodName(Type $param = default, ...)` -
/// confirmed directly against real `phpstan/phpdoc-parser`'s own
/// `parseMethodTagValue`/`parseMethodTagValueParameter`. The return-type-
/// vs-method-name ambiguity (both are bare-identifier-shaped) resolves
/// the same way real PHPStan's own parser does: try a type token first:
/// if it already contains its own `(...)` (no whitespace before the
/// parameter list at all), it was actually the *method name*, and there
/// is no return type; otherwise it's the return type, and the very next
/// token is the method name. A trailing free-text description (real,
/// common style) is simply never consumed past the parameter list's own
/// closing `)`, same as every other tag's own trailing-description text
/// elsewhere in this file.
fn parse_method_tag(rest: &str) -> Option<MethodTag> {
    let rest = rest.trim_start();
    let (is_static, rest) = match rest.strip_prefix("static") {
        Some(r) if r.starts_with(char::is_whitespace) => (true, r.trim_start()),
        _ => (false, rest),
    };
    let first = take_type_token(rest);
    if first.is_empty() {
        return None;
    }
    let (return_type, name_and_params) = if first.contains('(') {
        (None, first)
    } else {
        let remainder = rest[first.len()..].trim_start();
        let second = take_type_token(remainder);
        if second.is_empty() {
            return None;
        }
        (Some(first.to_string()), second)
    };
    let paren_idx = name_and_params.find('(')?;
    let name = name_and_params[..paren_idx].to_string();
    if name.is_empty() {
        return None;
    }
    let params_str = name_and_params[paren_idx + 1..].trim_end_matches(')');
    let params = if params_str.trim().is_empty() {
        Vec::new()
    } else {
        split_top_level_commas(params_str)
            .into_iter()
            .filter_map(parse_method_tag_param)
            .collect()
    };
    Some(MethodTag {
        is_static,
        return_type,
        name,
        params,
    })
}

/// One `@method` parameter - see `parse_method_tag`'s own doc comment.
/// The type-omitted case (`p` starting directly with `$`/`&`/`...`)
/// mirrors real PHPStan's own lookahead exactly (`parseMethodTagValueParameter`,
/// confirmed directly): a parameter type is only ever parsed when the
/// current token *isn't* already variable/reference/variadic-shaped.
fn parse_method_tag_param(p: &str) -> Option<MethodTagParam> {
    let p = p.trim();
    if p.is_empty() {
        return None;
    }
    let (ty, remainder) = if p.starts_with('$') || p.starts_with('&') || p.starts_with("...") {
        (None, p)
    } else {
        let t = take_type_token(p);
        (Some(t.to_string()), p[t.len()..].trim_start())
    };
    let by_ref = remainder.starts_with('&');
    let remainder = remainder.trim_start_matches('&').trim_start();
    let variadic = remainder.starts_with("...");
    let remainder = remainder.trim_start_matches("...").trim_start();
    let (name_part, has_default) = match remainder.split_once('=') {
        Some((n, _)) => (n.trim(), true),
        None => (remainder.trim(), false),
    };
    let name = name_part.strip_prefix('$')?.to_string();
    if name.is_empty() {
        return None;
    }
    Some(MethodTagParam {
        ty,
        name,
        by_ref,
        variadic,
        has_default,
    })
}

#[cfg(test)]
mod tests;
