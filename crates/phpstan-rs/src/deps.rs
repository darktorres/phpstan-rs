use rustc_hash::FxHashSet;

use syntax::ast::*;
use syntax::name_resolution::NameContext;
use syntax::visit::{
    Visitor, walk_class_const, walk_class_def, walk_function_decl, walk_param, walk_property_decl,
    walk_property_hooks, walk_stmt, walk_stmts,
};

/// Every class, function and constant one file's parse tree could
/// reference, as the keys `dependency_key` gives - used purely to decide
/// which files might need re-checking when a declaration changes. Missing
/// a real reference would let a stale diagnostic survive an edit
/// elsewhere, while an extra name costs one harmless re-check, so this
/// errs toward over-collecting: an unqualified function or constant name
/// in a namespace yields both the namespaced and the global name, and the
/// names in PHPDoc tags are taken from the tag text without parsing it.
pub fn referenced_names(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collector = Collector::new(true, false);
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// `referenced_names` and `declaration_referenced_names`, in one walk.
pub fn both_referenced_names(stmts: &[Stmt]) -> (FxHashSet<String>, FxHashSet<String>) {
    let mut collector = Collector::new(true, true);
    walk_stmts(&mut collector, stmts);
    (collector.names, collector.outside)
}

/// `referenced_names` without what only function and method bodies
/// mention: the names a file's declarations can take their shape from
/// (signatures, hierarchies, PHPDoc, constant values).
pub fn declaration_referenced_names(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collector = Collector::new(false, false);
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// The key a declaration's or reference's fully-qualified name (with or
/// without its leading `\`) is tracked under: PHP matches class and
/// function names case-insensitively.
pub fn dependency_key(name: &str) -> String {
    name.trim_start_matches('\\').to_ascii_lowercase()
}

/// The key a `global $name;` statement is tracked under, for the files
/// whose checking reads the project-wide type inferred for `$name`. A `$`
/// keeps it apart from every class, function and constant key.
pub fn global_variable_key(name: &str) -> String {
    format!("${name}")
}

/// Collects every name a file mentions, resolved against the `namespace`
/// and `use` statements in effect, anywhere in the file (nested scopes
/// included).
struct Collector {
    names: FxHashSet<String>,
    // With `track_outside`, the names found anywhere but in a function or
    // method body: what `declaration_referenced_names` finds, from the same
    // walk.
    outside: FxHashSet<String>,
    track_outside: bool,
    // How many function and method bodies the walk is inside.
    body_depth: usize,
    ctx: NameContext,
    // Whether function and method bodies are walked.
    bodies: bool,
    // The template parameters declared by the classes and functions being
    // walked, innermost last: names that are not classes.
    templates: Vec<FxHashSet<String>>,
}

impl Collector {
    fn new(bodies: bool, track_outside: bool) -> Collector {
        Collector {
            names: FxHashSet::default(),
            outside: FxHashSet::default(),
            track_outside,
            body_depth: 0,
            ctx: NameContext::default(),
            bodies,
            templates: Vec::new(),
        }
    }

    /// Records `key` as referenced.
    fn add(&mut self, key: String) {
        if self.track_outside && self.body_depth == 0 {
            self.outside.insert(key.clone());
        }
        self.names.insert(key);
    }

    /// The names a doc comment's tags mention, each resolved as a class
    /// and also taken as written (a constant-named type's global
    /// fallback). Only the types of a tag are read, not its description,
    /// and a template parameter in scope isn't a class.
    fn doc_names(&mut self, doc: Option<&str>) {
        let Some(doc) = doc else { return };
        let tags = doc_tags(doc);
        let declared = declared_templates(&tags);
        for (tag, rest) in &tags {
            for token in tag_type_tokens(tag, rest) {
                if declared.contains(token) || self.templates.iter().any(|t| t.contains(token)) {
                    continue;
                }
                let resolved = dependency_key(&self.ctx.resolve(token));
                self.add(resolved);
                self.add(dependency_key(token));
            }
        }
    }

    /// The template parameters `doc` declares.
    fn templates_of(doc: Option<&str>) -> FxHashSet<String> {
        doc.map(|doc| declared_templates(&doc_tags(doc)))
            .unwrap_or_default()
    }
}

/// A doc comment's tags: each tag's name (without its `@`) and what
/// follows it on its line.
fn doc_tags(doc: &str) -> Vec<(&str, &str)> {
    doc.lines()
        .filter_map(|line| {
            let line = line
                .trim_start()
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_start();
            let tag = line.strip_prefix('@')?;
            let end = tag
                .find(|c: char| c.is_whitespace() || c == '(')
                .unwrap_or(tag.len());
            Some((&tag[..end], &tag[end..]))
        })
        .collect()
}

/// A tag's name without a `phpstan-` or `psalm-` prefix.
fn tag_base(tag: &str) -> &str {
    tag.strip_prefix("phpstan-")
        .or_else(|| tag.strip_prefix("psalm-"))
        .unwrap_or(tag)
}

/// The names `@template` tags declare.
fn declared_templates(tags: &[(&str, &str)]) -> FxHashSet<String> {
    tags.iter()
        .filter(|(tag, _)| {
            matches!(
                tag_base(tag),
                "template" | "template-covariant" | "template-contravariant"
            )
        })
        .filter_map(|(_, rest)| rest.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// The name-shaped tokens in the types of one tag. A tag whose grammar is
/// known is read only where it holds types; one that isn't is read whole.
fn tag_type_tokens<'a>(tag: &str, rest: &'a str) -> Vec<&'a str> {
    let mut types: Vec<&'a str> = Vec::new();
    match tag_base(tag) {
        "param"
        | "param-out"
        | "param-immediately-invoked-callable"
        | "param-later-invoked-callable"
        | "param-closure-this"
        | "return"
        | "throws"
        | "property"
        | "property-read"
        | "property-write"
        | "mixin"
        | "extends"
        | "implements"
        | "use"
        | "template-extends"
        | "template-implements"
        | "template-use"
        | "this"
        | "self-out"
        | "if-this-is"
        | "assert"
        | "assert-if-true"
        | "assert-if-false" => {
            types.push(type_head(rest).0);
        }
        "var" => {
            let (head, after) = type_head(rest);
            types.push(head);
            // `@var $x Type`
            let after = after.trim_start();
            if after.starts_with('$') {
                let after = after.trim_start_matches(|c: char| !c.is_whitespace());
                types.push(type_head(after).0);
            }
        }
        "template" | "template-covariant" | "template-contravariant" => {
            let mut words = rest.trim_start();
            words = words.trim_start_matches(|c: char| !c.is_whitespace());
            loop {
                let trimmed = words.trim_start();
                let Some(after) = ["of ", "as ", "super ", "= "]
                    .iter()
                    .find_map(|keyword| trimmed.strip_prefix(keyword))
                else {
                    break;
                };
                let (head, remainder) = type_head(after);
                types.push(head);
                words = remainder;
            }
        }
        "type" => {
            if let Some((_, definition)) = rest.split_once('=') {
                types.push(type_head(definition).0);
            }
        }
        "import-type" => {
            if let Some((_, from)) = rest.split_once(" from ") {
                types.push(type_head(from).0);
            }
        }
        "deprecated" | "see" | "link" | "author" | "since" | "version" | "internal" | "todo"
        | "example" | "copyright" | "license" | "package" | "subpackage" | "category" | "api"
        | "inheritdoc" | "covers" => {}
        _ => types.push(rest),
    }
    types.into_iter().flat_map(name_tokens).collect()
}

/// The type expression starting `text` and what follows it: up to the
/// first space outside brackets, unless it continues (`|`, `&`, `:`).
fn type_head(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut end = text.len();
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '<' | '(' | '[' | '{') => depth += 1,
            (None, '>' | ')' | ']' | '}') => depth = depth.saturating_sub(1),
            (None, c) if c.is_whitespace() && depth == 0 => {
                let before = text[..i].chars().next_back();
                let after = text[i..].trim_start().chars().next();
                let continues =
                    matches!(before, Some('|' | '&' | ':')) || matches!(after, Some('|' | '&'));
                if !continues {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    (&text[..end], &text[end..])
}

/// The name-shaped tokens of `text`: runs of identifier characters and
/// `\` starting with a letter, `_` or `\`, not preceded by `$`.
fn name_tokens(text: &str) -> impl Iterator<Item = &str> {
    let is_name_char =
        |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '\\' || !c.is_ascii();
    let mut tokens = Vec::new();
    let mut start = None;
    let mut prev = ' ';
    // Where the hyphenated type keyword being passed over ends.
    let mut skip_until = 0;
    for (i, c) in text.char_indices() {
        if i < skip_until {
            prev = c;
            continue;
        }
        match start {
            Some(_) if is_name_char(c) => {}
            Some(s) => {
                tokens.push((&text[s..i], &text[i..]));
                start = None;
            }
            None if is_name_char(c) && !c.is_ascii_digit() && prev != '$' => {
                // `non-empty-string` is one keyword, not the names `non`,
                // `empty` and `string`.
                let end = hyphenated_end(text, i, is_name_char);
                if text[i..end].contains('-') && types::is_reserved_type_word(&text[i..end]) {
                    skip_until = end;
                } else {
                    start = Some(i);
                }
            }
            None => {}
        }
        prev = c;
    }
    if let Some(s) = start {
        tokens.push((&text[s..], ""));
    }
    tokens
        .into_iter()
        // An array shape's key (`array{a: int}`), not a class; `Foo::BAR`
        // is a class.
        .filter(|(_, after)| {
            let after = after.trim_start();
            !((after.starts_with(':') && !after.starts_with("::")) || after.starts_with("?:"))
        })
        .map(|(token, _)| token.trim_end_matches('\\'))
        .filter(|t| !t.trim_start_matches('\\').is_empty())
        // `int`, `string`, `array` and the other words a type is read from
        // are never a class: nothing declared can change what they name.
        .filter(|t| !types::is_reserved_type_word(t))
}

/// Where the run of name characters and hyphens joining them (a hyphen
/// between two letters) that starts at `start` ends.
fn hyphenated_end(text: &str, start: usize, is_name_char: impl Fn(char) -> bool) -> usize {
    let mut chars = text[start..].char_indices().peekable();
    let mut end = start;
    while let Some((offset, c)) = chars.next() {
        let joins = is_name_char(c)
            || (c == '-'
                && chars
                    .peek()
                    .is_some_and(|(_, next)| next.is_ascii_alphabetic()));
        if !joins {
            break;
        }
        end = start + offset + c.len_utf8();
    }
    end
}

impl<'ast> Visitor<'ast> for Collector {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        match &stmt.kind {
            StmtKind::Namespace(name) => self.ctx.set_namespace(name),
            StmtKind::Use(imports) => {
                self.ctx.add_uses(imports);
                for import in imports {
                    self.add(dependency_key(&import.path));
                }
            }
            StmtKind::Global(vars) => {
                for name in vars.iter().filter_map(|v| v.name()) {
                    self.add(global_variable_key(name));
                }
            }
            _ => {}
        }
        self.doc_names(stmt.doc_comment.as_deref());
        walk_stmt(self, stmt);
    }

    fn visit_function_decl(&mut self, function: &'ast FunctionDecl) {
        self.doc_names(function.doc_comment.as_deref());
        self.templates
            .push(Collector::templates_of(function.doc_comment.as_deref()));
        if self.bodies && self.track_outside {
            // The parts that are not the body count as outside it.
            for attr in &function.attrs {
                self.visit_attribute(attr);
            }
            for param in &function.params {
                self.visit_param(param);
            }
            if let Some(hint) = &function.return_type {
                self.visit_type_hint(hint);
            }
            self.body_depth += 1;
            walk_stmts(self, function.body_stmts());
            self.body_depth -= 1;
        } else if self.bodies {
            walk_function_decl(self, function);
        } else {
            for attr in &function.attrs {
                self.visit_attribute(attr);
            }
            for param in &function.params {
                self.visit_param(param);
            }
            if let Some(hint) = &function.return_type {
                self.visit_type_hint(hint);
            }
        }
        self.templates.pop();
    }

    fn visit_class_def(&mut self, class: &'ast ClassDef) {
        self.doc_names(class.doc_comment.as_deref());
        self.templates
            .push(Collector::templates_of(class.doc_comment.as_deref()));
        for trait_use in &class.trait_uses {
            self.doc_names(trait_use.doc_comment.as_deref());
        }
        walk_class_def(self, class);
        self.templates.pop();
    }

    fn visit_property_decl(&mut self, property: &'ast PropertyDecl) {
        self.doc_names(property.doc_comment.as_deref());
        walk_property_decl(self, property);
    }

    fn visit_class_const(&mut self, constant: &'ast ClassConst) {
        self.doc_names(constant.doc_comment.as_deref());
        walk_class_const(self, constant);
    }

    fn visit_param(&mut self, param: &'ast Param) {
        self.doc_names(param.doc_comment.as_deref());
        walk_param(self, param);
    }

    fn visit_property_hooks(&mut self, hooks: &'ast PropertyHooks) {
        for hook in hooks.iter() {
            self.doc_names(hook.doc.as_deref());
        }
        walk_property_hooks(self, hooks);
    }

    /// Every class, function and constant reference - in expressions,
    /// declarations' `extends`/`implements`/`use`, catch clauses,
    /// attributes and type declarations.
    fn visit_name(&mut self, name: &'ast QualifiedName) {
        match &name.resolved {
            ResolvedName::Exact(resolved) => {
                self.add(dependency_key(resolved));
            }
            ResolvedName::WithGlobalFallback(resolved) => {
                self.add(dependency_key(resolved));
                self.add(dependency_key(&name.written));
            }
            ResolvedName::Unresolved => {
                if !["self", "parent", "static"]
                    .iter()
                    .any(|keyword| name.is_keyword(keyword))
                {
                    self.add(dependency_key(&name.written));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
