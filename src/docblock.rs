use std::collections::{HashMap, HashSet};

/// A parsed subset of a `/** ... */` doc comment: just the tags this
/// checker can use (`@param`, `@return`, `@var`, ...). Types are kept as
/// raw strings here, resolved through `types::parse_type_string` only
/// where a caller needs a `Type` - this module doesn't depend on `types`
/// at all.
///
/// Deliberately does not parse conditional return types - those need
/// real per-call-site type parameter binding, not just parsing.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DocBlock {
    pub params: HashMap<String, String>,
    // `@param-out Type $name` - the widened type a by-ref parameter's
    // value is guaranteed to hold once the function returns.
    pub param_out: HashMap<String, String>,
    pub return_type: Option<String>,
    pub var_type: Option<String>,
    // The `$name` half of `@var Type $name`, when present - used when an
    // inline `@var` (checker::stmt) names a variable other than the
    // statement's own single assignment target.
    pub var_name: Option<String>,
    // `@template T` names, in declaration order (positional binding -
    // see `registry::ClassInfo::templates`).
    pub templates: Vec<String>,
    // `@template T of Bound` - the bound text, when declared.
    pub template_bounds: HashMap<String, String>,
    // `@template T = Default` - the default-type text, used when an
    // "erased" reference leaves this name unbound instead of falling
    // back to `Mixed`.
    pub template_defaults: HashMap<String, String>,
    // `@template-covariant`/`@template-contravariant` - a name absent
    // from both stays invariant, this project's default.
    pub covariant_templates: HashSet<String>,
    pub contravariant_templates: HashSet<String>,
    // Names of `@param`s tagged `[optional]` - marks a native parameter
    // as conceptually optional with no `= value` default (phpstorm-stubs
    // models several functions, e.g. `mt_rand`'s `$max`, only as their
    // highest-arity overload this way).
    pub optional_params: HashSet<String>,
    // `@extends Parent<Arg, ...>` - fixes the parent class's own
    // `@template` names. At most one is meaningful (a class has a single
    // native `extends`), so the last seen wins.
    pub extends: Option<String>,
    // `@implements Iface<Arg, ...>` - a class can `implements` several
    // interfaces, so every tag is kept.
    pub implements: Vec<String>,
    // `@use Trait<Arg, ...>` - same idea, for a generic trait in this
    // class's own `use` statement(s).
    pub uses: Vec<String>,
    // `@property`/`@property-read`/`@property-write Type $name` - magic
    // properties, typically backed by `__get`/`__set`.
    pub properties: Vec<PropertyTag>,
    // Real PHPStan's own backward-compatibility-promise marker - an
    // `@api`-tagged class/interface/trait is safe for a third-party
    // extension author to reference. Consumed by `checker::phpstan_api`,
    // gated on the project actually depending on `phpstan/phpstan`.
    pub api: bool,
    // `@api-do-not-implement` - an `@api` interface/class marked this way
    // may gain new abstract members in a minor version, so third-party
    // code must not `implements`/`extends` it. Implies `api`.
    pub api_do_not_implement: bool,
    // `@internal` - usable only from within its own declaring "root"
    // namespace. Consumed by `checker::internal_usage`, gated behind
    // `Registry::check_internal_usage` (default off, matching real
    // PHPStan's `featureToggles.internalTag: false`).
    pub internal: bool,
    // `@phpstan-consistent-constructor` - every subclass's own
    // `__construct` must match this class's exactly (real PHP itself
    // exempts `__construct` from ordinary override compatibility).
    // Consumed by `Registry::find_consistent_constructor_ancestor`.
    pub consistent_constructor: bool,
    // `@readonly` (also `@phpstan-readonly`) - the docblock-only
    // precursor to PHP 8.1's native `readonly`, folded into
    // `PropertySig::is_readonly` at `ClassInfo::from_decl` time so it
    // flows through the same enforcement/variance/override machinery a
    // native `readonly` property gets.
    // `@phpstan-readonly-allow-private-mutation`'s narrower exemption
    // isn't modeled.
    pub readonly: bool,
    // `@phpstan-immutable` - class-level only. `ClassReflection::
    // isImmutable()` treats this OR `@phpstan-readonly` on the class
    // identically; `ClassInfo::from_decl`/`from_enum_decl` compute
    // `is_immutable` as `immutable || readonly`.
    pub immutable: bool,
    // `@final` (also `@phpstan-final`) - declares a method/property
    // `final` without real PHP's native keyword. Consumed by
    // `ClassInfo::final_methods_by_phpdoc`/`final_properties_by_phpdoc`.
    pub is_final: bool,
    // `@phpstan-pure`/`@phpstan-impure` - checked against this project's
    // own computed purity classification rather than trusted outright
    // (`checker::noop::check_declared_purity_tag`). `pure` is parsed but
    // not currently consumed.
    pub pure: bool,
    pub impure: bool,
    // `@deprecated` - `None` when absent; `Some(message)` (possibly
    // empty) when present. This is the PHPDoc *tag*, distinct from PHP
    // 8.4's native `#[\Deprecated]` *attribute* (which emits a real
    // runtime warning), not handled here. Consumed by
    // `checker::deprecated_usage`, gated behind
    // `Registry::check_deprecated_usage` (default off).
    pub deprecated: Option<String>,
    // `@throws Type[, description]` - the checked-exception type(s) a
    // caller should be prepared for; every tag is kept and consumed as a
    // union. Resolved to a real `Type` (and validated as `Throwable`) at
    // registry-build time.
    pub throws: Vec<String>,
    // `@see Target[ description]` - kept exactly as written (a plain
    // symbol reference, a URL, or free text; distinguishing those is
    // `checker::check_doc_see_tag_targets`'s job, not parsing's - real
    // PHPStan's own `phpstan/phpdoc-parser` has no `SeeTagValueNode` at
    // all, so there's no upstream shape to defer to here either).
    pub see: Vec<String>,
    // `@phpstan-assert[-if-true/-if-false] [!]Type $target` - real
    // PHPStan's custom-assertion-function convention. `assert` narrows
    // unconditionally; `assert_if_true`/`assert_if_false` only once this
    // declaration's return value is known. `=Type`/`!=value` forms
    // (template-/value-equality assertions) are skipped rather than
    // misparsed.
    pub assert_if_true: Vec<AssertTag>,
    pub assert_if_false: Vec<AssertTag>,
    pub assert: Vec<AssertTag>,
    // `@phpstan-type Name TypeExpr` - a local type alias, referenced bare
    // from that same class's `@param`/`@return`/`@var` tags. Resolved
    // into a real `Type` at `ClassInfo::from_decl` time.
    pub type_aliases: HashMap<String, String>,
    // `@phpstan-import-type Name from Class [as Alias]` - imports another
    // class's own local alias. Resolved once
    // `Registry::resolve_deferred_type_imports` can see every class's
    // already-built `type_aliases` table (the source class may not be
    // indexed yet at `from_decl` time).
    pub type_imports: Vec<TypeImportTag>,
    // `@phpstan-require-extends Type` - declared on a trait/interface,
    // requiring every using/implementing class to be a subtype of
    // `Type`. Checked by `checker::oop::check_require_tags`.
    pub require_extends: Vec<String>,
    // `@phpstan-require-implements Type` - a trait's sibling of
    // `require_extends`, requiring `implements` instead (real PHPStan
    // doesn't recognize this on an interface, which can't `implements`).
    pub require_implements: Vec<String>,
    // `@phpstan-sealed Type` - the *complete* list of types allowed to
    // directly `extends`/`implements` this one. Unlike
    // `require_extends`/`require_implements` (transitive, checked
    // against every consumer in the hierarchy), this is a one-level,
    // exact-name allowlist checked against every *immediate* child only
    // (`checker::oop::check_sealed_hierarchy`).
    pub sealed: Vec<String>,
    // `@method [static] ReturnType methodName(Type $param = default, ...)`
    // - a virtual method with no real declaration (conventionally backed
    // by `__call`/`__callStatic`). Kept parsed, not raw text, since the
    // grammar is more than one type expression - see `parse_method_tag`.
    pub method_tags: Vec<MethodTag>,
    // `@mixin Type` - every `Type`'s own *public* method/property becomes
    // available on this class too (conventionally forwarded via
    // `__call`/`__get`). Resolved at `Registry::resolve_deferred_mixins`
    // time (the target may not be indexed yet at `from_decl` time).
    pub mixins: Vec<String>,
    // `@param-immediately-invoked-callable $name`/
    // `@param-later-invoked-callable $name` - merged into one map keyed
    // by parameter name, `true` for immediately-invoked. Declaration-time
    // validity is checked by
    // `checker::noop::check_declared_param_callable_tags`; the runtime
    // scope-threading effect these drive in real PHPStan isn't modeled.
    pub param_immediately_invoked_callable: HashMap<String, bool>,
    // `@param-closure-this Type $name` - the type `$this` becomes inside
    // a `Closure`-typed parameter's own literal argument. Seeds `$this`
    // via `checker::calls::check_closure_literal_with_this_and_scope`,
    // the same mechanism `Closure::bind`/`->bindTo()` use - see
    // `registry::function_sig::ParamSig::closure_this_type`.
    pub param_closure_this: HashMap<String, String>,
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
        // trailing `// description` (e.g. `@var array{v?: string, //
        // null, undefined, ..., b?: bool, // boolean, ...}`). Without
        // stripping it, the comment text gets spliced straight into the
        // joined type string below and `take_type_token` happily parses
        // "// null" as a bare class-like name, corrupting the whole
        // shape - `//` never appears in type-expression syntax, so
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

    // PHPStan recognizes a `@phpstan-`-prefixed sibling of `@param`/
    // `@return`/`@var`/`@template*`/`@extends`/`@implements` as a more
    // precise, PHPStan-only override of the same information - meant to
    // coexist with a plainer tag for non-PHPStan tooling (e.g. a native
    // `@return` staying the loose `array` an IDE/other tools understand
    // while `@phpstan-return list<T>` carries the precise generic type),
    // not replace it outright. Two passes over the same tag list - plain tags
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

/// Extracts a PHPDoc-conventional `$name` token - the variable-name half
/// of `@param Type $name description`/`@var Type $name`/`@property Type
/// $name`/... every tag handler below that needs one shares this same
/// shape. Strips a leading `&` (by-ref, e.g. `@param string[] &$matches
/// [optional]`, common PHP-manual style) and `...` (variadic, `@param
/// int ...$nums` - without this, `strip_prefix('$')` fails on the
/// literal `.`), plus a trailing `:` some PHPDoc runs the description
/// straight into with no separating space (`@param Type $name:
/// description`) - `split_whitespace` alone leaves that colon attached
/// to the extracted name, silently storing/looking up the tag under a
/// name (`"post:"`) the real parameter (`"post"`) never matches,
/// discarding its own doc type entirely.
fn extract_doc_var_name(token: &str) -> Option<&str> {
    token
        .trim_start_matches('&')
        .trim_start_matches("...")
        .strip_prefix('$')
        .map(|name| name.trim_end_matches(':'))
}

fn apply_tag(block: &mut DocBlock, tag: &str, rest: &str) {
    {
        match tag {
            "param" => {
                // Convention is `Type $name description...`.
                let ty = take_type_token(rest);
                let remainder = rest[ty.len()..].trim_start();
                let var_token = remainder.split_whitespace().next().unwrap_or("");
                if let Some(name) = extract_doc_var_name(var_token) {
                    if !ty.is_empty() {
                        block.params.insert(name.to_string(), ty.to_string());
                    }
                    let after_name = remainder[var_token.len()..].trim_start();
                    if after_name.starts_with("[optional]") {
                        block.optional_params.insert(name.to_string());
                    }
                }
            }
            "param-out" => {
                // Identical shape to `"param"` above (minus its own
                // `[optional]` marker, meaningless for an output type).
                let ty = take_type_token(rest);
                let remainder = rest[ty.len()..].trim_start();
                let var_token = remainder.split_whitespace().next().unwrap_or("");
                if let Some(name) = extract_doc_var_name(var_token)
                    && !ty.is_empty()
                {
                    block.param_out.insert(name.to_string(), ty.to_string());
                }
            }
            "return" => {
                let ty = take_type_token(rest);
                // Conditional return types (`(\$param
                // is Foo ? Bar : Baz)`, real PHPStan syntax Doctrine
                // DBAL's own `DriverManager::getConnection()` uses) used
                // to be deliberately excluded here (`!ty.starts_with
                // ("($")`) - this project had no per-call-site binding for
                // them, and `types::parse_type_string` collapsed one to
                // an eager union of both branches, discarding the
                // condition entirely, so setting `return_type` from one
                // was worse than leaving it unset. Now that `Type::
                // Conditional`/`resolve_conditional` exist and do real
                // per-call-site resolution, that exclusion is gone -
                // `take_type_token`'s own balanced-parens grab is exactly
                // what `try_parse_conditional_type` needs to see the
                // whole `(...)` token intact.
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
                        .and_then(extract_doc_var_name)
                    {
                        block.var_name = Some(name.to_string());
                    }
                }
            }
            "param-immediately-invoked-callable" | "param-later-invoked-callable" => {
                // Grammar is just `$paramName [description]` - no type
                // token, unlike `@param`.
                if let Some(name) = rest
                    .split_whitespace()
                    .next()
                    .and_then(extract_doc_var_name)
                {
                    block.param_immediately_invoked_callable.insert(
                        name.to_string(),
                        tag == "param-immediately-invoked-callable",
                    );
                }
            }
            "param-closure-this" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    let remainder = rest[ty.len()..].trim_start();
                    if let Some(name) = remainder
                        .split_whitespace()
                        .next()
                        .and_then(extract_doc_var_name)
                    {
                        block
                            .param_closure_this
                            .insert(name.to_string(), ty.to_string());
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
                        .and_then(extract_doc_var_name)
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
            // Same shape as a plain `@template`, plus which direction
            // the parameter is allowed to vary in.
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
                // A common variant of this tag's syntax writes an `=`
                // between the alias name and its type expression
                // (`@phpstan-type Alias = ConcreteType`) - without
                // stripping it, `take_type_token` stops at the first
                // whitespace and takes the bare `=` itself as the whole
                // "type", later resolved into a bogus namespace-qualified
                // class name ending in `\=`.
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
                // keyword grammar, not a type expression, so plain
                // whitespace-splitting is enough.
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
            // `@template-extends`/`@template-implements` are real
            // PHPStan-recognized synonyms for plain `@extends`/
            // `@implements`, used throughout phpstorm-stubs itself
            // (`Generator`'s own `@template-implements
            // Iterator<TKey, TYield>`) - without this,
            // `implements_args["Iterator"]` would be silently empty, and
            // `Registry::class_template_bindings(_, _, "Iterator")` on
            // any `Generator<...>`-typed value would zip its real type
            // arguments against nothing.
            "extends" | "template-extends" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.extends = Some(ty.to_string());
                }
            }
            "implements" | "template-implements" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.implements.push(ty.to_string());
                }
            }
            // Real PHPStan-recognized synonym for `@use`, mirroring
            // `@extends`/`@template-extends` and `@implements`/
            // `@template-implements` just above. `@phpstan-use` needs no
            // separate arm: `parse`'s own two-pass loop already strips a
            // leading `phpstan-` off every tag.
            "use" | "template-use" => {
                let ty = take_type_token(rest);
                if !ty.is_empty() {
                    block.uses.push(ty.to_string());
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
            // See `DocBlock::api`/`api_do_not_implement`'s own
            // doc comments. Neither tag takes an argument, so `rest` is
            // never consulted - unlike every other arm above.
            "api" => {
                block.api = true;
            }
            "api-do-not-implement" => {
                block.api = true;
                block.api_do_not_implement = true;
            }
            // See `DocBlock::internal`'s own doc comment. Same
            // no-argument shape as `@api` just above.
            "internal" => {
                block.internal = true;
            }
            "consistent-constructor" => {
                block.consistent_constructor = true;
            }
            "readonly" => {
                block.readonly = true;
            }
            "immutable" => {
                block.immutable = true;
            }
            "final" => {
                block.is_final = true;
            }
            "pure" => {
                block.pure = true;
            }
            "impure" => {
                block.impure = true;
            }
            // See `DocBlock::deprecated`'s own doc comment.
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
            "see" => {
                let target = take_type_token(rest);
                if !target.is_empty() {
                    block.see.push(target.to_string());
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
/// or `T of Bound = Default` with both together, in that
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
/// `"..."` string-literal segment (a legal PHPStan literal-string-union
/// member, e.g. `' '|'>'|'+'|'~'`) is tracked as one opaque run too -
/// without this, a space inside the quotes is wrongly treated as the
/// token's end, cutting the whole type down to a bare, dangling `'`.
fn take_type_token(s: &str) -> &str {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    // True from the moment a `)` brings `depth` back to 0 *and*
    // a `:` genuinely follows it (skipping whitespace) - a `Closure(...):
    // ReturnType`/`callable(...): ReturnType` shape's own trailing return
    // type sits outside the parens, separated from them by this `:`, and
    // the plain depth-0-whitespace-stops rule below would otherwise
    // truncate right after the `:` (`@param Closure(int): string $fn`
    // would lose everything from " string" onward, silently degrading to
    // the bare `Closure(int):` fragment). Decided once, right at the `)`,
    // via a bounded lookahead - not a blanket
    // "tolerate whitespace after any `)`" rule, which would wrongly
    // swallow an *ordinary* parenthesized type's own real terminating
    // space too (`(A&B) $x` must still stop right after its own `)`,
    // since nothing but the next token follows it). Cleared the moment
    // the first real (non-whitespace) return-type character is consumed,
    // so a *later* space - the real terminator before e.g. `$fn` - still
    // stops normally; recurses correctly for a nested callable-shape
    // return type too (`Closure(int): Closure(string): bool`), since
    // each `)` closing back to depth 0 re-triggers its own lookahead.
    let mut in_return_type_transition = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                in_return_type_transition = false;
            }
            '<' | '{' | '(' | '[' => {
                depth += 1;
                in_return_type_transition = false;
            }
            ')' => {
                depth -= 1;
                in_return_type_transition =
                    depth == 0 && s[i + c.len_utf8()..].trim_start().starts_with(':');
            }
            '>' | '}' | ']' => {
                depth -= 1;
                in_return_type_transition = false;
            }
            ':' if in_return_type_transition => {}
            c if c.is_whitespace() && depth == 0 => {
                if in_return_type_transition {
                    continue;
                }
                return &s[..i];
            }
            _ => {
                in_return_type_transition = false;
            }
        }
    }
    s
}

/// Splits `s` on `,`, but only where it appears outside any `<>`/`{}`/
/// `()`/`[]` nesting - `@method`'s parameter list needs this (e.g.
/// `array<string, mixed> $options = []`, whose internal comma must not
/// split the parameter list itself). Mirrors `take_type_
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
/// mirrors real `phpstan/phpdoc-parser`'s own `parseMethodTagValue`/
/// `parseMethodTagValueParameter`. The return-type-vs-method-name
/// ambiguity (both are bare-identifier-shaped) resolves the same way:
/// try a type token first:
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

/// One `@method` parameter - see `parse_method_tag`. The type-omitted
/// case (`p` starting directly with `$`/`&`/`...`) mirrors real
/// PHPStan's own lookahead (`parseMethodTagValueParameter`): a parameter
/// type is only ever parsed when the current token isn't already
/// variable/reference/variadic-shaped.
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

/// Every `@phpstan-*` tag name real PHPStan itself recognizes - `real
/// PhpDoc/Tag`'s own inventory, transcribed directly (`InvalidPHPStanDocTagRule
/// ::POSSIBLE_PHPSTAN_TAGS`, `vendor/phpstan-src`, not guessed) - the
/// prefix itself stripped, matching every other tag-name comparison in
/// this file. `@phpstan-all-methods-pure`/`-impure` (a real, if rare,
/// class-level purity-tag pair this project's own `apply_tag` doesn't
/// otherwise parse - the transitive-purity slice never needed
/// a declared-intent tag, only computed purity) are kept in this list
/// purely so a real, valid use of either doesn't get wrongly flagged as
/// unknown - not a claim that this project consumes them for anything.
const KNOWN_PHPSTAN_TAGS: &[&str] = &[
    "param",
    "param-out",
    "var",
    "extends",
    "implements",
    "use",
    "template",
    "template-contravariant",
    "template-covariant",
    "return",
    "throws",
    "ignore",
    "ignore-next-line",
    "ignore-line",
    "method",
    "pure",
    "impure",
    "immutable",
    "type",
    "import-type",
    "property",
    "property-read",
    "property-write",
    "consistent-constructor",
    "assert",
    "assert-if-true",
    "assert-if-false",
    "self-out",
    "this-out",
    "allow-private-mutation",
    "readonly",
    "readonly-allow-private-mutation",
    "require-extends",
    "require-implements",
    "sealed",
    "param-immediately-invoked-callable",
    "param-later-invoked-callable",
    "param-closure-this",
    "all-methods-pure",
    "all-methods-impure",
];

/// `@phpstan-*`-prefixed tag *names* found in `raw` that aren't in
/// `KNOWN_PHPSTAN_TAGS` above - real PHPStan's own `InvalidPHPStanDocTagRule`
/// (`phpDoc.phpstanTag`), a real, common way to catch a typo'd tag
/// (`@phpstan-parm`) silently doing nothing instead of what was intended.
/// A plain text scan, not routed through the rest of this module's own
/// tag-line-joining machinery - deliberately so: that machinery exists
/// to extract a tag's own *value*, but this only ever needs the tag
/// *names* actually written, recognized or not, including one this
/// module's own `apply_tag` would otherwise silently drop on the floor
/// (its only way to report "you wrote a tag we don't understand" at
/// all). A real, disclosed imprecision: a literal `@phpstan-` substring
/// inside a description/code-example rather than a real tag position
/// would also match - accepted as a corner case real PHPStan's own
/// tokenizing lexer doesn't share, but not expected to occur in
/// practice (a docblock talking *about* a tag almost always still writes
/// it at the start of its own line, the position that matters for this
/// scan too, just not enforced separately here).
pub(crate) fn unknown_phpstan_tags(raw: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut rest = raw;
    while let Some(idx) = rest.find("@phpstan-") {
        let after = &rest[idx + "@phpstan-".len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or(after.len());
        let tag = &after[..end];
        if !tag.is_empty() && !KNOWN_PHPSTAN_TAGS.contains(&tag) {
            result.push(tag.to_string());
        }
        rest = &after[end..];
    }
    result
}

#[cfg(test)]
mod tests;
