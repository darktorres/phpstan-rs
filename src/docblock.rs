use std::collections::{HashMap, HashSet};

use crate::phpdoc_grammar::{self, PhpDocParseError, PhpDocTagValue};

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
    // `@phpstan-readonly-allow-private-mutation` sets this too, along
    // with `allow_private_mutation` below.
    pub readonly: bool,
    // `@phpstan-allow-private-mutation`/`@phpstan-readonly-allow-private-
    // mutation` - a `@readonly` property its own class may still write.
    pub allow_private_mutation: bool,
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
    // `@phpstan-self-out Type`/`@phpstan-this-out Type` (and the
    // `@psalm-` spellings): the type the receiver has after a call to
    // this method.
    pub self_out: Option<String>,
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
    // Every real, recoverable parse error `phpdoc_grammar` produced while
    // parsing this docblock's own type expressions - real PHPStan's own
    // `phpDoc.parseError`, structurally available now that Stage B (see
    // `phpdoc_grammar`'s own module doc comment) sources every type-
    // bearing tag's type from that grammar instead of the old bracket-
    // depth-only `take_type_token` scan. Not yet consumed by any checker
    // rule - collected here so a future rule can be, without another
    // parsing pass.
    pub parse_errors: Vec<PhpDocParseError>,
    // One entry per plain/`@phpstan-` tag whose value produced at least one
    // `parse_errors` entry - the tag-level view real PHPStan's
    // `InvalidPhpDocTagValueRule` (`phpDoc.parseError`) reports. Foreign
    // `@psalm-`/`@phan-` tags never land here: an unparsable one is skipped
    // outright (`foreign_tag_has_invalid_type`), and real PHPStan's rule
    // skips those prefixes too.
    pub invalid_tag_values: Vec<InvalidTagValue>,
}

/// One tag whose value failed to parse - see `DocBlock::invalid_tag_values`.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidTagValue {
    // The tag name exactly as written, `@` excluded (`param`,
    // `phpstan-return`).
    pub tag: String,
    pub value: String,
    pub message: String,
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
    /// `@method T name<T of Bound = Default>(...)`'s own templates, as
    /// `(name, bound, default)` source text.
    pub templates: Vec<(String, Option<String>, Option<String>)>,
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
/// comparison, not a real type check anyway).
fn parse_assert_tag(rest: &str) -> (Option<AssertTag>, Vec<PhpDocParseError>) {
    let rest = rest.trim();
    if rest.starts_with("!=") || rest.starts_with('=') {
        return (None, Vec::new());
    }
    let (negated, rest) = match rest.strip_prefix('!') {
        Some(after) => (true, after),
        None => (false, rest),
    };
    let (ty, target, errors) = phpdoc_grammar::parse_type_prefix(rest);
    let Some(ty) = ty else {
        return (None, errors);
    };
    let ty = ty.to_source_string();
    let target = target.trim();
    if ty.is_empty() || target.is_empty() {
        return (None, errors);
    }
    (
        Some(AssertTag {
            negated,
            ty,
            target: target.to_string(),
        }),
        errors,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTag {
    pub name: String,
    pub ty: String,
    pub readable: bool,
    pub writable: bool,
}

#[hotpath::measure]
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
        // Continuation lines join with a real newline, not a space: a
        // wrapped array-shape field's own trailing `// description`
        // (`@var array{v?: string, // null, ...`) is a comment only up to
        // the end of its own line, which `phpdoc_grammar`'s lexer skips
        // the same way phpdoc-parser's `TOKEN_COMMENT` does. Stripping
        // `//`-onward here instead would also cut `http://...` out of a
        // description or a `@see` URL.
        if line.starts_with('@') || tag_lines.is_empty() {
            tag_lines.push(line.to_string());
        } else if let Some(last) = tag_lines.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }

    // A prefixed sibling of a plain tag (`@phpstan-return list<T>` next to
    // a looser `@return array`) is a more precise, tool-specific override
    // of the same information, meant to coexist with the plain tag other
    // tools understand. One pass per spelling, in real PHPStan's own
    // `PhpDocNodeResolver` precedence order - plain, then `@phan-`, then
    // `@psalm-`, then `@phpstan-` - with every later pass overwriting an
    // earlier one through the exact same per-tag logic, gives the most
    // specific spelling its right to win regardless of which one the
    // docblock happens to write first.
    for line in &tag_lines {
        let Some(tag_body) = line.strip_prefix('@') else {
            continue;
        };
        if tag_body.starts_with("phpstan-")
            || tag_body.starts_with("psalm-")
            || tag_body.starts_with("phan-")
        {
            continue;
        }
        let (tag, rest) = split_tag(tag_body);
        // Only the prefixed spelling means anything to PHPStan (a bare
        // `@assert (1, 2) == 3` is PHPUnit's own annotation).
        if PREFIXED_ONLY_TAGS_PHPSTAN_PARSES.contains(&tag) {
            continue;
        }
        apply_tag_recording_errors(&mut block, tag, tag, rest);
    }
    // The assert family is the one exception to "later spelling
    // overwrites": `resolveAssertTags` takes the first prefix among
    // `@phpstan`/`@psalm`/`@phan` that has any assert tag at all and
    // ignores the rest, so `@psalm-`/`@phan-` asserts are collected aside
    // and only used when nothing earlier in that order supplied one.
    let mut foreign_asserts = [DocBlock::default(), DocBlock::default()];
    for (prefix, asserts) in [("phan-", 1), ("psalm-", 0)] {
        for line in &tag_lines {
            let Some(tag_body) = line.strip_prefix('@').and_then(|b| b.strip_prefix(prefix)) else {
                continue;
            };
            let (tag, rest) = split_tag(tag_body);
            let Some(canonical) = foreign_tag_canonical_name(prefix, tag) else {
                continue;
            };
            if foreign_tag_has_invalid_type(canonical, rest) {
                continue;
            }
            if canonical.starts_with("assert") {
                apply_tag(&mut foreign_asserts[asserts], canonical, rest);
            } else {
                apply_tag(&mut block, canonical, rest);
            }
        }
    }
    for line in &tag_lines {
        let Some(written) = line.strip_prefix('@') else {
            continue;
        };
        let Some(tag_body) = written.strip_prefix("phpstan-") else {
            continue;
        };
        let (tag, rest) = split_tag(tag_body);
        apply_tag_recording_errors(&mut block, split_tag(written).0, tag, rest);
    }
    let has_assert = |b: &DocBlock| {
        !(b.assert.is_empty() && b.assert_if_true.is_empty() && b.assert_if_false.is_empty())
    };
    if !has_assert(&block)
        && let Some(source) = foreign_asserts.into_iter().find(has_assert)
    {
        block.assert = source.assert;
        block.assert_if_true = source.assert_if_true;
        block.assert_if_false = source.assert_if_false;
    }
    // Templates keep the order their names first appear in, whatever
    // spelling wins (real PHPStan's `chooseTemplateTagValueNodesByPriority`
    // overrides a name in place): `@phpstan-template TKey` then
    // `@template-covariant T` is `<TKey, T>`, not plain tags first.
    let first_seen = |name: &str| {
        tag_lines.iter().position(|line| {
            let Some(body) = line.strip_prefix('@') else {
                return false;
            };
            let body = ["phpstan-", "psalm-", "phan-"]
                .iter()
                .find_map(|p| body.strip_prefix(p))
                .unwrap_or(body);
            let (tag, rest) = split_tag(body);
            tag.starts_with("template") && rest.split_whitespace().next() == Some(name)
        })
    };
    block
        .templates
        .sort_by_key(|name| first_seen(name).unwrap_or(usize::MAX));

    block
}

/// `apply_tag`, plus one `DocBlock::invalid_tag_values` entry when this
/// tag's value added any new `parse_errors`. `written` is the tag name as
/// it appears in the source (prefix included), `tag` the name `apply_tag`
/// dispatches on.
///
/// A tag real PHPStan parses and finds invalid is an `InvalidTagValueNode`
/// there - it contributes nothing, not whatever a recovering parser could
/// salvage from it (`@return array(width, height)` gives no return type,
/// not `array`). So the tag is applied to a scratch block first and only
/// merged in when it parsed cleanly; its errors are kept either way.
fn apply_tag_recording_errors(block: &mut DocBlock, written: &str, tag: &str, rest: &str) {
    // A tag phpdoc-parser has no dedicated parser for, whose value opens
    // with `(`, is a Doctrine annotation there (`@ORM\Column(...)`,
    // `@url (image.jpg) ...`), invalid when its argument list doesn't
    // parse. This project has nothing to apply from such a tag either way.
    let is_prefixed = written.starts_with("phpstan-");
    let has_dedicated_parser = PLAIN_TAGS_PHPSTAN_PARSES.contains(&tag)
        || (is_prefixed && PREFIXED_ONLY_TAGS_PHPSTAN_PARSES.contains(&tag));
    if !has_dedicated_parser && rest.starts_with('(') {
        let errors = phpdoc_grammar::doctrine_argument_errors(rest);
        if let Some(first) = errors.first() {
            block.invalid_tag_values.push(InvalidTagValue {
                tag: written.to_string(),
                value: rest.to_string(),
                message: first.message.clone(),
            });
            block.parse_errors.extend(errors);
        }
    }
    let mut scratch = DocBlock::default();
    apply_tag(&mut scratch, tag, rest);
    let parsed_by_phpstan =
        written.starts_with("phpstan-") || PLAIN_TAGS_PHPSTAN_PARSES.contains(&tag);
    if parsed_by_phpstan && let Some(first) = scratch.parse_errors.first() {
        block.invalid_tag_values.push(InvalidTagValue {
            tag: written.to_string(),
            value: rest.to_string(),
            message: first.message.clone(),
        });
        block.parse_errors.append(&mut scratch.parse_errors);
        return;
    }
    block.merge_from(scratch);
}

impl DocBlock {
    /// Folds one tag's own scratch block (`apply_tag_recording_errors`)
    /// into `self` with exactly the semantics `apply_tag` would have had
    /// applying the tag to `self` directly: a single-valued field is
    /// overwritten when the tag set it, a map/set gains the tag's entries,
    /// a list is appended (templates deduplicated by name), a flag is
    /// OR-ed. Destructured exhaustively so a new `DocBlock` field is a
    /// compile error here until it's handled.
    fn merge_from(&mut self, other: DocBlock) {
        let DocBlock {
            params,
            param_out,
            return_type,
            var_type,
            var_name,
            templates,
            template_bounds,
            template_defaults,
            covariant_templates,
            contravariant_templates,
            optional_params,
            extends,
            implements,
            uses,
            properties,
            api,
            api_do_not_implement,
            internal,
            consistent_constructor,
            readonly,
            allow_private_mutation,
            immutable,
            is_final,
            pure,
            impure,
            deprecated,
            throws,
            see,
            assert_if_true,
            assert_if_false,
            assert,
            type_aliases,
            type_imports,
            require_extends,
            require_implements,
            sealed,
            method_tags,
            mixins,
            self_out,
            param_immediately_invoked_callable,
            param_closure_this,
            parse_errors,
            invalid_tag_values,
        } = other;
        self.params.extend(params);
        self.param_out.extend(param_out);
        if return_type.is_some() {
            self.return_type = return_type;
        }
        if var_type.is_some() {
            self.var_type = var_type;
        }
        if var_name.is_some() {
            self.var_name = var_name;
        }
        for name in templates {
            if !self.templates.contains(&name) {
                self.templates.push(name);
            }
        }
        self.template_bounds.extend(template_bounds);
        self.template_defaults.extend(template_defaults);
        self.covariant_templates.extend(covariant_templates);
        self.contravariant_templates.extend(contravariant_templates);
        self.optional_params.extend(optional_params);
        if extends.is_some() {
            self.extends = extends;
        }
        self.implements.extend(implements);
        self.uses.extend(uses);
        self.properties.extend(properties);
        self.api |= api;
        self.api_do_not_implement |= api_do_not_implement;
        self.internal |= internal;
        self.consistent_constructor |= consistent_constructor;
        self.readonly |= readonly;
        self.allow_private_mutation |= allow_private_mutation;
        self.immutable |= immutable;
        self.is_final |= is_final;
        self.pure |= pure;
        self.impure |= impure;
        if deprecated.is_some() {
            self.deprecated = deprecated;
        }
        self.throws.extend(throws);
        self.see.extend(see);
        self.assert_if_true.extend(assert_if_true);
        self.assert_if_false.extend(assert_if_false);
        self.assert.extend(assert);
        self.type_aliases.extend(type_aliases);
        self.type_imports.extend(type_imports);
        self.require_extends.extend(require_extends);
        self.require_implements.extend(require_implements);
        self.sealed.extend(sealed);
        self.method_tags.extend(method_tags);
        self.mixins.extend(mixins);
        if self_out.is_some() {
            self.self_out = self_out;
        }
        self.param_immediately_invoked_callable
            .extend(param_immediately_invoked_callable);
        self.param_closure_this.extend(param_closure_this);
        self.parse_errors.extend(parse_errors);
        self.invalid_tag_values.extend(invalid_tag_values);
    }
}

/// The plain (unprefixed) tag names real phpdoc-parser's `parseTagValue`
/// gives a structured value, and can therefore report as invalid. A plain
/// `@type`/`@assert`/`@import-type`/`@require-extends`/`@sealed` (which
/// `apply_tag` still honors, for other tools' conventions) is only ever a
/// generic tag to real PHPStan, never a parse error - only its
/// `@phpstan-` spelling is parsed.
const PLAIN_TAGS_PHPSTAN_PARSES: &[&str] = &[
    "param",
    "param-out",
    "param-immediately-invoked-callable",
    "param-later-invoked-callable",
    "param-closure-this",
    "var",
    "return",
    "throws",
    "mixin",
    "deprecated",
    "property",
    "property-read",
    "property-write",
    "method",
    "template",
    "template-covariant",
    "template-contravariant",
    "extends",
    "template-extends",
    "implements",
    "template-implements",
    "use",
    "template-use",
];

/// A tag line's name and value. The name runs up to the first character
/// that can't be part of one - phpdoc-parser's own tag tokens stop there
/// too, so `@ORM\Column(type="string")` is the tag `ORM\Column` with the
/// value `(type="string")`, not one run-on name.
fn split_tag(tag_body: &str) -> (&str, &str) {
    let end = tag_body
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '\\' | ':')))
        .unwrap_or(tag_body.len());
    (&tag_body[..end], tag_body[end..].trim())
}

/// The prefixed-only tag names phpdoc-parser gives a dedicated parser
/// (only their `@phpstan-` form - see `PLAIN_TAGS_PHPSTAN_PARSES`).
const PREFIXED_ONLY_TAGS_PHPSTAN_PARSES: &[&str] = &[
    "type",
    "import-type",
    "assert",
    "assert-if-true",
    "assert-if-false",
    "self-out",
    "this-out",
    "require-extends",
    "require-implements",
    "sealed",
    "pure-unless-callable-is-impure",
];

/// The `apply_tag` name a `@psalm-`/`@phan-`-prefixed tag maps onto -
/// exactly the spellings real PHPStan's `PhpDocNodeResolver` resolves,
/// transcribed from its own per-tag lists. Anything else under those
/// prefixes is Psalm/Phan-only vocabulary with no PHPStan meaning
/// (`@psalm-suppress`, `@psalm-internal` - a namespace argument, not
/// `@internal`'s no-argument form - `@psalm-mutation-free`, ...) and is
/// skipped.
fn foreign_tag_canonical_name(prefix: &str, tag: &str) -> Option<&'static str> {
    let canonical = match (prefix, tag) {
        (_, "var") => "var",
        (_, "param") => "param",
        (_, "return") => "return",
        (_, "method") => "method",
        (_, "template") => "template",
        (_, "type") => "type",
        (_, "pure") => "pure",
        (_, "immutable") => "immutable",
        (_, "assert") => "assert",
        (_, "assert-if-true") => "assert-if-true",
        (_, "assert-if-false") => "assert-if-false",
        ("phan-", "real-return") => "return",
        ("phan-", "extends" | "inherits") => "extends",
        ("phan-", "side-effect-free") => "pure",
        ("phan-", "read-only") => "readonly",
        ("psalm-", "template-covariant") => "template-covariant",
        ("psalm-", "template-contravariant") => "template-contravariant",
        ("psalm-", "param-out") => "param-out",
        ("psalm-", "import-type") => "import-type",
        ("psalm-", "require-extends") => "require-extends",
        ("psalm-", "require-implements") => "require-implements",
        ("psalm-", "inheritors") => "sealed",
        ("psalm-", "readonly") => "readonly",
        ("psalm-", "allow-private-mutation") => "allow-private-mutation",
        ("psalm-", "readonly-allow-private-mutation") => "readonly-allow-private-mutation",
        ("psalm-", "consistent-constructor") => "consistent-constructor",
        ("psalm-", "self-out" | "this-out") => "self-out",
        _ => return None,
    };
    Some(canonical)
}

/// Real PHPStan skips a `@psalm-`-prefixed tag whose type it can't
/// resolve (`PhpDocNodeResolver::shouldSkipType`), so Psalm-only type
/// syntax degrades to "tag absent" - the plain tag still applies -
/// instead of producing an error type. This project's nearest
/// equivalent at the docblock layer is the tag's own type text failing
/// to parse, applied to `@phan-` tags too since the same reasoning
/// holds for Phan-only syntax.
fn foreign_tag_has_invalid_type(canonical: &str, rest: &str) -> bool {
    match canonical {
        "var"
        | "param"
        | "param-out"
        | "return"
        | "method"
        | "template"
        | "template-covariant"
        | "template-contravariant"
        | "extends" => !phpdoc_grammar::parse_tag_rest(canonical, rest).1.is_empty(),
        "type" | "require-extends" | "require-implements" | "sealed" => {
            let remainder = if canonical == "type" {
                rest.split_once(char::is_whitespace)
                    .map_or("", |(_, r)| r.trim_start())
                    .trim_start_matches('=')
                    .trim_start()
            } else {
                rest
            };
            let (ty, _, errors) = phpdoc_grammar::parse_type_prefix(remainder);
            ty.is_none() || !errors.is_empty()
        }
        "assert" | "assert-if-true" | "assert-if-false" => {
            let (tag, errors) = parse_assert_tag(rest);
            tag.is_none() || !errors.is_empty()
        }
        _ => false,
    }
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
            // Every type-bearing tag below sources its type from
            // `phpdoc_grammar` - a real tokenizer + recursive-descent
            // parser (Phase 56 Stage A/B) - rather than the old `take_
            // type_token` bracket-depth-only scan, then re-stringifies
            // the parsed `TypeNode` back into this struct's own
            // `Option<String>` fields for full backward compatibility
            // with every existing consumer (`phpdoc_grammar`'s own
            // module doc comment). Any tag whose name `phpdoc_grammar`
            // gives a dedicated structured shape to (`PhpDocTagValue::
            // TypedVar`/`Type`/`Template`/`ExtendsLike`/`Method`) also
            // sources its variable name/by-ref/variadic/bound/default
            // from that same structure, not from a second, independent
            // re-derivation of the surrounding text.
            "param" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("param", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::TypedVar {
                    ty,
                    variable: Some(name),
                    description,
                    ..
                } = value
                {
                    if let Some(ty) = ty {
                        block.params.insert(name.clone(), ty.to_source_string());
                    }
                    // Tokens reconstruct with a space between every one
                    // of them (`remaining_description`'s own doc
                    // comment) - `"[optional]"` re-lexes as three
                    // separate tokens (`[`, `optional`, `]`), so
                    // stripping spaces before comparing is what makes
                    // this survive that reconstruction.
                    if description.replace(' ', "").starts_with("[optional]") {
                        block.optional_params.insert(name);
                    }
                }
            }
            "param-out" => {
                // Identical shape to `"param"` above (minus its own
                // `[optional]` marker, meaningless for an output type).
                let (value, errors) = phpdoc_grammar::parse_tag_rest("param-out", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::TypedVar {
                    ty: Some(ty),
                    variable: Some(name),
                    ..
                } = value
                {
                    block.param_out.insert(name, ty.to_source_string());
                }
            }
            "return" => {
                // Conditional return types (`(\$param
                // is Foo ? Bar : Baz)`, real PHPStan syntax Doctrine
                // DBAL's own `DriverManager::getConnection()` uses) used
                // to be deliberately excluded here - this project had no
                // per-call-site binding for them, and `types::
                // parse_type_string` collapsed one to an eager union of
                // both branches, discarding the condition entirely, so
                // setting `return_type` from one was worse than leaving
                // it unset. Now that `Type::Conditional`/
                // `resolve_conditional` exist and do real per-call-site
                // resolution, that exclusion is gone -
                // `phpdoc_grammar`'s own `parse_paren_or_conditional`
                // parses the whole `(...)` shape as a real `TypeNode::
                // Conditional`, and `to_source_string` re-emits it in
                // exactly the `" is "`/`"?"`/`":"` shape `types::
                // parse_type_string`'s own conditional parser expects.
                let (value, errors) = phpdoc_grammar::parse_tag_rest("return", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::Type { ty: Some(ty), .. } = value {
                    block.return_type = Some(ty.to_source_string());
                }
            }
            "var" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("var", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::TypedVar { ty, variable, .. } = value {
                    if let Some(ty) = ty {
                        block.var_type = Some(ty.to_source_string());
                    }
                    if let Some(name) = variable {
                        block.var_name = Some(name);
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
                // Not one of `phpdoc_grammar`'s own dedicated tag shapes
                // (`TYPED_VAR_TAGS`) - just one leading type expression
                // followed by `$name`, same shape `parse_type_prefix`
                // exists for.
                let (ty, remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty
                    && let Some(name) = remainder
                        .split_whitespace()
                        .next()
                        .and_then(extract_doc_var_name)
                {
                    block
                        .param_closure_this
                        .insert(name.to_string(), ty.to_source_string());
                }
            }
            "property" | "property-read" | "property-write" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest(tag, rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::TypedVar {
                    ty: Some(ty),
                    variable: Some(name),
                    ..
                } = value
                {
                    block.properties.push(PropertyTag {
                        name,
                        ty: ty.to_source_string(),
                        readable: tag != "property-write",
                        writable: tag != "property-read",
                    });
                }
            }
            "template" | "template-covariant" | "template-contravariant" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest(tag, rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::Template {
                    name,
                    bound,
                    default,
                } = value
                    && !name.is_empty()
                {
                    if !block.templates.contains(&name) {
                        block.templates.push(name.clone());
                    }
                    if let Some(bound) = bound {
                        block
                            .template_bounds
                            .insert(name.clone(), bound.to_source_string());
                    }
                    if let Some(default) = default {
                        block
                            .template_defaults
                            .insert(name.clone(), default.to_source_string());
                    }
                    match tag {
                        "template-covariant" => {
                            block.covariant_templates.insert(name);
                        }
                        "template-contravariant" => {
                            block.contravariant_templates.insert(name);
                        }
                        _ => {}
                    }
                }
            }
            "type" => {
                let mut parts = rest.splitn(2, char::is_whitespace);
                let name = parts.next().unwrap_or("");
                let remainder = parts.next().unwrap_or("").trim_start();
                // A common variant of this tag's syntax writes an `=`
                // between the alias name and its type expression
                // (`@phpstan-type Alias = ConcreteType`) - without
                // stripping it, the type parser sees a bare `=` as its
                // first token and errors out instead of parsing the real
                // type that follows.
                let remainder = remainder
                    .strip_prefix('=')
                    .unwrap_or(remainder)
                    .trim_start();
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(remainder);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    let ty = ty.to_source_string();
                    if !name.is_empty() && !ty.is_empty() {
                        block.type_aliases.insert(name.to_string(), ty);
                    }
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
            // arguments against nothing. `phpdoc_grammar` only knows the
            // plain names, so the synonym is mapped onto its bare
            // counterpart right here rather than taught to the grammar
            // itself.
            "extends" | "template-extends" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("extends", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::ExtendsLike { ty } = value {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.extends = Some(ty);
                    }
                }
            }
            "implements" | "template-implements" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("implements", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::ExtendsLike { ty } = value {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.implements.push(ty);
                    }
                }
            }
            // Real PHPStan-recognized synonym for `@use`, mirroring
            // `@extends`/`@template-extends` and `@implements`/
            // `@template-implements` just above. `@phpstan-use` needs no
            // separate arm: `parse`'s own two-pass loop already strips a
            // leading `phpstan-` off every tag.
            "use" | "template-use" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("use", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::ExtendsLike { ty } = value {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.uses.push(ty);
                    }
                }
            }
            "require-extends" => {
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.require_extends.push(ty);
                    }
                }
            }
            "require-implements" => {
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.require_implements.push(ty);
                    }
                }
            }
            "sealed" => {
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.sealed.push(ty);
                    }
                }
            }
            "method" => {
                let (value, errors) = phpdoc_grammar::parse_tag_rest("method", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::Method {
                    is_static,
                    return_type,
                    name,
                    templates,
                    params,
                } = value
                    && !name.is_empty()
                {
                    block.method_tags.push(MethodTag {
                        is_static,
                        return_type: return_type.map(|t| t.to_source_string()),
                        name,
                        templates: templates
                            .into_iter()
                            .map(|t| {
                                (
                                    t.name,
                                    t.bound.map(|b| b.to_source_string()),
                                    t.default.map(|d| d.to_source_string()),
                                )
                            })
                            .collect(),
                        params: params
                            .into_iter()
                            .map(|p| MethodTagParam {
                                ty: p.ty.map(|t| t.to_source_string()),
                                name: p.name,
                                by_ref: p.by_ref,
                                variadic: p.variadic,
                                has_default: p.has_default,
                            })
                            .collect(),
                    });
                }
            }
            "self-out" | "this-out" => {
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    block.self_out = Some(ty.to_source_string());
                }
            }
            "mixin" => {
                let (ty, _remainder, errors) = phpdoc_grammar::parse_type_prefix(rest);
                block.parse_errors.extend(errors);
                if let Some(ty) = ty {
                    let ty = ty.to_source_string();
                    if !ty.is_empty() {
                        block.mixins.push(ty);
                    }
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
            "allow-private-mutation" => {
                block.allow_private_mutation = true;
            }
            "readonly-allow-private-mutation" => {
                block.readonly = true;
                block.allow_private_mutation = true;
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
                let (value, errors) = phpdoc_grammar::parse_tag_rest("throws", rest);
                block.parse_errors.extend(errors);
                if let PhpDocTagValue::Type { ty: Some(ty), .. } = value {
                    block.throws.push(ty.to_source_string());
                }
            }
            // Not routed through `phpdoc_grammar` at all - a `@see`
            // target is a plain symbol reference, a URL, or free text,
            // never a real type expression (real PHPStan's own
            // `phpstan/phpdoc-parser` has no `SeeTagValueNode` either -
            // see `DocBlock::see`'s own doc comment) - the type grammar's
            // stricter parsing would only misparse it.
            "see" => {
                let target = take_type_token(rest);
                if !target.is_empty() {
                    block.see.push(target.to_string());
                }
            }
            "assert-if-true" => {
                let (tag, errors) = parse_assert_tag(rest);
                block.parse_errors.extend(errors);
                if let Some(tag) = tag {
                    block.assert_if_true.push(tag);
                }
            }
            "assert-if-false" => {
                let (tag, errors) = parse_assert_tag(rest);
                block.parse_errors.extend(errors);
                if let Some(tag) = tag {
                    block.assert_if_false.push(tag);
                }
            }
            "assert" => {
                let (tag, errors) = parse_assert_tag(rest);
                block.parse_errors.extend(errors);
                if let Some(tag) = tag {
                    block.assert.push(tag);
                }
            }
            _ => {}
        }
    }
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
