//! The module-graph gate: every module of the `phpstan-rs` package is
//! assigned the crate it is headed for, and no module may refer to a module
//! assigned to a crate its own crate may not depend on, unless the edge is
//! listed as a known exception together with the phase that removes it.
//!
//! The assignments, the crate dependency rules and the exceptions live in
//! `gates/module-graph.txt`:
//!
//! ```text
//! crate <name> [<dependency> ...]
//! module <module::path> <crate>
//! allow <from::module> <to::module> <phase>
//! ```
//!
//! A module belongs to the longest `module` entry that is a prefix of its
//! path, and an `allow` line names two such entries. Every module must be
//! assigned, and every `allow` line must still match an edge, so the list
//! only shrinks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::Visit;

const SPEC: &str = "gates/module-graph.txt";
const SRC: &str = "crates/phpstan-rs/src";

type ModPath = Vec<String>;

struct Spec {
    deps: BTreeMap<String, BTreeSet<String>>,
    /// `module` rows: path -> crate.
    modules: BTreeMap<ModPath, String>,
    /// (from row, to row) -> phase.
    allowed: BTreeMap<(ModPath, ModPath), String>,
}

struct SourceFile {
    path: PathBuf,
    module: ModPath,
    ast: syn::File,
}

struct Edge {
    from: ModPath,
    to: ModPath,
    file: PathBuf,
    line: usize,
}

pub fn modgraph(root: &Path, args: &[String]) -> Result<(), String> {
    let inventory = args.iter().any(|a| a == "--inventory");
    let spec = read_spec(&root.join(SPEC))?;
    let files = parse_files(&root.join(SRC))?;

    let mut known = BTreeSet::new();
    for file in &files {
        known.insert(file.module.clone());
        collect_inline_modules(&file.module, &file.ast.items, &mut known);
    }

    let resolver = Resolver::new(&known, &files);
    let mut edges = Vec::new();
    for file in &files {
        let mut walker = Walker {
            resolver: &resolver,
            module: file.module.clone(),
            file: &file.path,
            edges: &mut edges,
            impls: Vec::new(),
        };
        walker.items(&file.ast.items);
    }

    let mut problems = Vec::new();
    for module in &known {
        if spec.row(module).is_none() {
            problems.push(format!("module {} has no assignment", join(module)));
        }
    }
    // (from row, to row) -> (count, first site)
    let mut crossings: BTreeMap<(ModPath, ModPath), (usize, String)> = BTreeMap::new();
    for edge in &edges {
        let (Some((from_row, from_crate)), Some((to_row, to_crate))) =
            (spec.row(&edge.from), spec.row(&edge.to))
        else {
            continue;
        };
        if from_crate == to_crate || spec.deps[from_crate].contains(to_crate) {
            continue;
        }
        let entry = crossings
            .entry((from_row.clone(), to_row.clone()))
            .or_insert_with(|| {
                let file = edge.file.strip_prefix(root).unwrap_or(&edge.file);
                (0, format!("{}:{}", file.display(), edge.line))
            });
        entry.0 += 1;
    }

    if inventory {
        for ((from, to), (count, site)) in &crossings {
            let phase = spec
                .allowed
                .get(&(from.clone(), to.clone()))
                .map_or("-", String::as_str);
            println!(
                "{} ({}) -> {} ({}) x{count} [{phase}] {site}",
                join(from),
                spec.modules[from],
                join(to),
                spec.modules[to]
            );
        }
    }

    for ((from, to), (count, site)) in &crossings {
        if !spec.allowed.contains_key(&(from.clone(), to.clone())) {
            problems.push(format!(
                "{} ({}) -> {} ({}): {count} reference(s), first at {site}",
                join(from),
                spec.modules[from],
                join(to),
                spec.modules[to]
            ));
        }
    }
    for (from, to) in spec.allowed.keys() {
        if !crossings.contains_key(&(from.clone(), to.clone())) {
            problems.push(format!(
                "allow {} {}: no such edge any more; remove the line",
                join(from),
                join(to)
            ));
        }
    }
    if problems.is_empty() {
        eprintln!(
            "module graph: {} modules, {} references, {} listed exceptions",
            known.len(),
            edges.len(),
            spec.allowed.len()
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("{p}");
        }
        Err(format!("module graph: {} problem(s)", problems.len()))
    }
}

impl Spec {
    /// The longest `module` row that is a prefix of `path`, and its crate.
    fn row(&self, path: &[String]) -> Option<(&ModPath, &String)> {
        self.modules
            .iter()
            .filter(|(m, _)| path.starts_with(m))
            .max_by_key(|(m, _)| m.len())
    }
}

fn read_spec(path: &Path) -> Result<Spec, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut spec = Spec {
        deps: BTreeMap::new(),
        modules: BTreeMap::new(),
        allowed: BTreeMap::new(),
    };
    let mut allows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        let words: Vec<&str> = line.split_whitespace().collect();
        let bad = || format!("{}:{}: malformed line", path.display(), i + 1);
        match words.as_slice() {
            [] => {}
            ["crate", name, deps @ ..] => {
                let deps = deps.iter().map(|d| d.to_string()).collect();
                if spec.deps.insert(name.to_string(), deps).is_some() {
                    return Err(format!("{}:{}: crate {name} twice", path.display(), i + 1));
                }
            }
            ["module", module, krate] => {
                if spec
                    .modules
                    .insert(split(module), krate.to_string())
                    .is_some()
                {
                    return Err(format!(
                        "{}:{}: module {module} twice",
                        path.display(),
                        i + 1
                    ));
                }
            }
            ["allow", from, to, phase] => allows.push((i + 1, *from, *to, *phase)),
            _ => return Err(bad()),
        }
    }
    for (krate, deps) in &spec.deps {
        for dep in deps {
            if !spec.deps.contains_key(dep) {
                return Err(format!("crate {krate} depends on unknown crate {dep}"));
            }
        }
    }
    for (module, krate) in &spec.modules {
        if !spec.deps.contains_key(krate) {
            return Err(format!(
                "module {} assigned to unknown crate {krate}",
                join(module)
            ));
        }
    }
    for (line, from, to, phase) in allows {
        let (from, to) = (split(from), split(to));
        for m in [&from, &to] {
            if !spec.modules.contains_key(m) {
                return Err(format!(
                    "{}:{line}: {} is not a module row",
                    path.display(),
                    join(m)
                ));
            }
        }
        if spec.allowed.insert((from, to), phase.to_string()).is_some() {
            return Err(format!("{}:{line}: duplicate allow", path.display()));
        }
    }
    Ok(spec)
}

/// Every library source file with its module path. The binary's own
/// modules (`main.rs` and `main/`) are the `main` module.
fn parse_files(src: &Path) -> Result<Vec<SourceFile>, String> {
    let mut paths = Vec::new();
    walk_dir(src, &mut paths)?;
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        let rel = path.strip_prefix(src).expect("walked below src");
        let mut module: ModPath = rel
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if module.last().is_some_and(|m| m == "mod") {
            module.pop();
        }
        if module == ["lib"] {
            module.clear();
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let ast = syn::parse_file(&text).map_err(|e| {
            let start = e.span().start();
            format!("{}:{}:{}: {e}", path.display(), start.line, start.column)
        })?;
        files.push(SourceFile { path, module, ast });
    }
    Ok(files)
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            walk_dir(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn collect_inline_modules(module: &[String], items: &[syn::Item], known: &mut BTreeSet<ModPath>) {
    for item in items {
        if let syn::Item::Mod(m) = item
            && let Some((_, content)) = &m.content
        {
            let mut path = module.to_vec();
            path.push(m.ident.to_string());
            known.insert(path.clone());
            collect_inline_modules(&path, content, known);
        }
    }
}

/// A module's imports: named (`use a::b as c;` binds `c`) and glob.
#[derive(Default)]
struct Imports {
    named: Vec<(String, ModPath)>,
    globs: Vec<ModPath>,
}

/// Where inherent methods are defined: the engine calls rule code as
/// `self.check_x()` on a type whose `impl` blocks are spread over many
/// modules, so a call names the module that defines the method.
#[derive(Default)]
struct Methods {
    /// (type name, method) -> defining modules
    by_type: BTreeMap<(String, String), BTreeSet<ModPath>>,
    /// method -> (type name, defining module)
    by_name: BTreeMap<String, BTreeSet<(String, ModPath)>>,
    /// type name -> modules defining a type of that name
    types: BTreeMap<String, BTreeSet<ModPath>>,
}

impl Methods {
    /// The module defining `ty::name`, if exactly one does.
    fn on_type(&self, ty: &str, name: &str) -> Option<&ModPath> {
        let modules = self.by_type.get(&(ty.to_string(), name.to_string()))?;
        (modules.len() == 1).then(|| modules.first().expect("one"))
    }

    /// The module defining a method called on a receiver of unknown type:
    /// only a name defined once in the package, on a type whose inherent
    /// methods span several modules (so the name is distinctive).
    fn by_name(&self, name: &str, spread: &BTreeSet<String>) -> Option<&ModPath> {
        let defs = self.by_name.get(name)?;
        let (ty, module) = defs.first()?;
        (defs.len() == 1 && spread.contains(ty)).then_some(module)
    }
}

/// Resolves paths as written in one module to package module paths.
struct Resolver<'a> {
    known: &'a BTreeSet<ModPath>,
    scopes: BTreeMap<ModPath, BTreeMap<String, ModPath>>,
    methods: Methods,
    /// Types whose inherent methods are defined in more than one module.
    spread: BTreeSet<String>,
}

impl<'a> Resolver<'a> {
    fn new(known: &'a BTreeSet<ModPath>, files: &[SourceFile]) -> Self {
        let mut resolver = Resolver {
            known,
            scopes: BTreeMap::new(),
            methods: Methods::default(),
            spread: BTreeSet::new(),
        };
        let mut imports: BTreeMap<ModPath, Imports> = BTreeMap::new();
        let mut methods = Methods::default();
        for file in files {
            let mut collector = Collector {
                resolver: &resolver,
                module: file.module.clone(),
                imports: &mut imports,
                methods: &mut methods,
            };
            collector.items(&file.ast.items);
        }
        let mut modules_of: BTreeMap<&String, BTreeSet<&ModPath>> = BTreeMap::new();
        for ((ty, _), modules) in &methods.by_type {
            modules_of.entry(ty).or_default().extend(modules);
        }
        resolver.spread = modules_of
            .into_iter()
            .filter(|(ty, m)| m.len() > 1 && methods.types.get(*ty).is_some_and(|d| d.len() == 1))
            .map(|(ty, _)| ty.clone())
            .collect();
        resolver.methods = methods;
        for module in known {
            let mut visiting = BTreeSet::new();
            resolver.scope(module, &imports, &mut visiting);
        }
        resolver
    }

    fn children(&self, module: &[String]) -> impl Iterator<Item = &ModPath> {
        self.known
            .iter()
            .filter(move |m| m.len() == module.len() + 1 && m.starts_with(module))
    }

    /// The names in scope in `module`: its child modules, what its glob
    /// imports bring in, and its named imports, each shadowing the one
    /// before.
    fn scope(
        &mut self,
        module: &ModPath,
        imports: &BTreeMap<ModPath, Imports>,
        visiting: &mut BTreeSet<ModPath>,
    ) -> BTreeMap<String, ModPath> {
        if let Some(scope) = self.scopes.get(module) {
            return scope.clone();
        }
        let mut names = BTreeMap::new();
        if !visiting.insert(module.clone()) {
            return names;
        }
        for child in self.children(module) {
            names.insert(child.last().expect("non-empty").clone(), child.clone());
        }
        if let Some(own) = imports.get(module) {
            for glob in &own.globs {
                if self.known.contains(glob) {
                    for (name, path) in self.scope(glob, imports, visiting) {
                        names.entry(name).or_insert(path);
                    }
                }
            }
            for (name, path) in &own.named {
                names.insert(name.clone(), path.clone());
            }
        }
        visiting.remove(module);
        self.scopes.insert(module.clone(), names.clone());
        names
    }

    fn crate_root(module: &[String]) -> ModPath {
        if module.first().is_some_and(|m| m == "main") {
            vec!["main".to_string()]
        } else {
            Vec::new()
        }
    }

    /// Resolves a path that starts at `crate`, `self`, `super`, the library
    /// (from the binary) or, unless `use_path`, a name in scope. A `use`
    /// path only sees child modules, which is all its imports need here.
    fn resolve(&self, module: &[String], segments: &[String], use_path: bool) -> Option<ModPath> {
        let first = segments.first()?;
        let mut rest = &segments[1..];
        let mut path = match first.as_str() {
            "crate" => Self::crate_root(module),
            "phpstan_rs" => Vec::new(),
            "self" => module.to_vec(),
            "super" => {
                let mut path = module.to_vec();
                path.pop()?;
                while rest.first().is_some_and(|s| s == "super") {
                    path.pop()?;
                    rest = &rest[1..];
                }
                path
            }
            name if use_path => {
                let mut path = module.to_vec();
                path.push(name.to_string());
                if !self.known.contains(&path) {
                    return None;
                }
                path
            }
            name => self.scopes.get(module)?.get(name)?.clone(),
        };
        path.extend(rest.iter().cloned());
        Some(self.canonical(path))
    }

    /// Follows re-exports: `crate::Checker` is `crate::checker::Checker`
    /// when the root has `pub use checker::Checker`.
    fn canonical(&self, mut path: ModPath) -> ModPath {
        for _ in 0..16 {
            let mut changed = false;
            for i in 0..path.len() {
                let (module, name) = (&path[..i], &path[i]);
                if !self.known.contains(module) {
                    break;
                }
                let mut child = module.to_vec();
                child.push(name.clone());
                if self.known.contains(&child) {
                    continue;
                }
                if let Some(target) = self.scopes.get(module).and_then(|s| s.get(name))
                    && *target != child
                {
                    let mut next = target.clone();
                    next.extend(path[i + 1..].iter().cloned());
                    path = next;
                    changed = true;
                }
                break;
            }
            if !changed {
                break;
            }
        }
        path
    }

    /// Expands a `use` tree into (bound name or `None` for a glob, path).
    fn use_tree(
        &self,
        module: &[String],
        prefix: &[String],
        tree: &syn::UseTree,
        out: &mut Vec<(Option<String>, ModPath)>,
    ) {
        let with = |s: String| {
            let mut p = prefix.to_vec();
            p.push(s);
            p
        };
        match tree {
            syn::UseTree::Path(p) => {
                self.use_tree(module, &with(p.ident.to_string()), &p.tree, out);
            }
            syn::UseTree::Name(n) if n.ident == "self" => {
                if let Some(path) = self.resolve(module, prefix, true) {
                    out.push((prefix.last().cloned(), path));
                }
            }
            syn::UseTree::Name(n) => {
                let name = n.ident.to_string();
                if let Some(path) = self.resolve(module, &with(name.clone()), true) {
                    out.push((Some(name), path));
                }
            }
            syn::UseTree::Rename(r) => {
                if let Some(path) = self.resolve(module, &with(r.ident.to_string()), true) {
                    out.push((Some(r.rename.to_string()), path));
                }
            }
            syn::UseTree::Glob(_) => {
                if let Some(path) = self.resolve(module, prefix, true) {
                    out.push((None, path));
                }
            }
            syn::UseTree::Group(g) => {
                for t in &g.items {
                    self.use_tree(module, prefix, t, out);
                }
            }
        }
    }
}

/// Gathers every module's imports, including `use` items inside function
/// bodies, which count as the enclosing module's.
struct Collector<'r, 'a> {
    resolver: &'r Resolver<'a>,
    module: ModPath,
    imports: &'r mut BTreeMap<ModPath, Imports>,
    methods: &'r mut Methods,
}

impl Collector<'_, '_> {
    fn define(&mut self, ident: &syn::Ident) {
        self.methods
            .types
            .entry(ident.to_string())
            .or_default()
            .insert(self.module.clone());
    }

    fn items(&mut self, items: &[syn::Item]) {
        for item in items {
            match item {
                syn::Item::Mod(m) => {
                    if let Some((_, content)) = &m.content {
                        let saved = self.module.clone();
                        self.module.push(m.ident.to_string());
                        self.items(content);
                        self.module = saved;
                    }
                }
                _ => self.visit_item(item),
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector<'_, '_> {
    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        self.define(&i.ident);
        syn::visit::visit_item_struct(self, i);
    }

    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        self.define(&i.ident);
        syn::visit::visit_item_enum(self, i);
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        if i.trait_.is_none()
            && let Some(ty) = type_name(&i.self_ty)
        {
            for item in &i.items {
                if let syn::ImplItem::Fn(f) = item {
                    let name = f.sig.ident.to_string();
                    self.methods
                        .by_type
                        .entry((ty.clone(), name.clone()))
                        .or_default()
                        .insert(self.module.clone());
                    self.methods
                        .by_name
                        .entry(name)
                        .or_default()
                        .insert((ty.clone(), self.module.clone()));
                }
            }
        }
        syn::visit::visit_item_impl(self, i);
    }

    /// A trait's methods are definitions too: a name a trait also
    /// declares isn't distinctive of one type's inherent method.
    fn visit_item_trait(&mut self, t: &'ast syn::ItemTrait) {
        for item in &t.items {
            if let syn::TraitItem::Fn(f) = item {
                self.methods
                    .by_name
                    .entry(f.sig.ident.to_string())
                    .or_default()
                    .insert((t.ident.to_string(), self.module.clone()));
            }
        }
        syn::visit::visit_item_trait(self, t);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if let Some((_, content)) = &m.content {
            for item in content {
                self.visit_item(item);
            }
        }
    }

    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut out = Vec::new();
        self.resolver.use_tree(&self.module, &[], &u.tree, &mut out);
        let imports = self.imports.entry(self.module.clone()).or_default();
        for (name, path) in out {
            match name {
                Some(name) if name != "_" => imports.named.push((name, path)),
                Some(_) => {}
                None => imports.globs.push(path),
            }
        }
    }
}

/// Records every reference a module's code makes to a package module.
struct Walker<'r, 'a> {
    resolver: &'r Resolver<'a>,
    module: ModPath,
    file: &'r Path,
    edges: &'r mut Vec<Edge>,
    /// The `Self` type of each enclosing `impl`.
    impls: Vec<Option<String>>,
}

impl Walker<'_, '_> {
    fn items(&mut self, items: &[syn::Item]) {
        for item in items {
            match item {
                syn::Item::Mod(m) => {
                    if let Some((_, content)) = &m.content {
                        let saved = self.module.clone();
                        self.module.push(m.ident.to_string());
                        self.items(content);
                        self.module = saved;
                    }
                }
                _ => self.visit_item(item),
            }
        }
    }

    fn record(&mut self, to: ModPath, line: usize) {
        if to.is_empty() || to == self.module {
            return;
        }
        self.edges.push(Edge {
            from: self.module.clone(),
            to,
            file: self.file.to_path_buf(),
            line,
        });
    }

    fn path(&mut self, segments: &[String], line: usize) {
        if let [ty, method] = segments
            && ty == "Self"
        {
            if let Some(Some(ty)) = self.impls.last()
                && let Some(module) = self.resolver.methods.on_type(ty, method)
            {
                self.record(module.clone(), line);
            }
            return;
        }
        if let Some(path) = self.resolver.resolve(&self.module, segments, false) {
            // `Type::method` names the module defining the method.
            if let [.., ty, method] = path.as_slice()
                && let Some(module) = self.resolver.methods.on_type(ty, method)
            {
                self.record(module.clone(), line);
            }
            self.record(path, line);
        }
    }

    /// Paths inside macro input (`matches!`, `format!`, ...), found as
    /// `a::b::c` token runs.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees: Vec<TokenTree> = tokens.into_iter().collect();
        let mut i = 0;
        while i < trees.len() {
            match &trees[i] {
                TokenTree::Group(g) => {
                    self.tokens(g.stream());
                    i += 1;
                }
                TokenTree::Ident(first) => {
                    let line = first.span().start().line;
                    let mut segments = vec![first.to_string()];
                    let mut j = i + 1;
                    while let (
                        Some(TokenTree::Punct(a)),
                        Some(TokenTree::Punct(b)),
                        Some(TokenTree::Ident(next)),
                    ) = (trees.get(j), trees.get(j + 1), trees.get(j + 2))
                        && a.as_char() == ':'
                        && b.as_char() == ':'
                    {
                        segments.push(next.to_string());
                        j += 3;
                    }
                    if segments.len() > 1 {
                        self.path(&segments, line);
                    }
                    i = j;
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => i += 1,
            }
        }
    }
}

impl<'ast> Visit<'ast> for Walker<'_, '_> {
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        self.impls.push(type_name(&i.self_ty));
        syn::visit::visit_item_impl(self, i);
        self.impls.pop();
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let on_self = matches!(&*call.receiver, syn::Expr::Path(p) if p.path.is_ident("self"));
        let module = match self.impls.last() {
            Some(Some(ty)) if on_self => self.resolver.methods.on_type(ty, &name),
            _ => self.resolver.methods.by_name(&name, &self.resolver.spread),
        };
        if let Some(module) = module {
            self.record(module.clone(), call.method.span().start().line);
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_item_mod(&mut self, m: &'ast syn::ItemMod) {
        if let Some((_, content)) = &m.content {
            for item in content {
                self.visit_item(item);
            }
        }
    }

    fn visit_item_use(&mut self, u: &'ast syn::ItemUse) {
        let mut out = Vec::new();
        self.resolver.use_tree(&self.module, &[], &u.tree, &mut out);
        let line = u.span().start().line;
        for (_, path) in out {
            self.record(path, line);
        }
    }

    fn visit_path(&mut self, p: &'ast syn::Path) {
        if p.segments.len() > 1 {
            let segments: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
            self.path(&segments, p.span().start().line);
        }
        syn::visit::visit_path(self, p);
    }

    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        self.visit_path(&m.path);
        self.tokens(m.tokens.clone());
    }
}

/// The last path segment of an `impl`'s self type (`Checker<'a>` -> `Checker`).
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn split(s: &str) -> ModPath {
    if s == "crate" {
        Vec::new()
    } else {
        s.split("::").map(str::to_string).collect()
    }
}

fn join(p: &[String]) -> String {
    if p.is_empty() {
        "crate".to_string()
    } else {
        p.join("::")
    }
}
