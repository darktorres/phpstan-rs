use syntax::name::Name;
use syntax::name_resolution::NameContext;
use types::Type;

/// The class a class name written in code names; a name resolution
/// leaves unresolved (`self`, `parent`, `static`) as written.
pub(crate) fn class_ref(name: &syntax::ast::QualifiedName) -> Name {
    Name::from(name.resolved_class().unwrap_or(&name.written))
}

/// `ty`, a type as written (`types::parse_written_type`), with every
/// class name resolved as `ctx` resolves it.
pub(crate) fn resolve_names_in_type(ty: Type, ctx: &NameContext) -> Type {
    if !ty.has_class_names() {
        return ty;
    }
    // The same type is resolved in the same context again and again: by
    // each declaration of a file that writes it. What it resolves to is a
    // function of the type as written, the case of its class names
    // included, and the context's namespace and imports. So a type is
    // found by its node, which the entry holds, and not by equality, which
    // ignores that case.
    thread_local! {
        static RESOLVED: std::cell::RefCell<rustc_hash::FxHashMap<(usize, u64), (Type, Type)>> =
            std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    }
    const LIMIT: usize = 1 << 16;
    let version = ctx.version();
    if version != 0
        && let Some(done) = RESOLVED.with(|memo| {
            memo.borrow()
                .get(&(ty.node_addr(), version))
                .filter(|(input, _)| input.is_same_node(&ty))
                .map(|(_, resolved)| resolved.clone())
        })
    {
        return done;
    }
    let resolved = ty.map_class_names(&mut |name| ctx.resolve(name).into());
    if version != 0 {
        RESOLVED.with(|memo| {
            let mut memo = memo.borrow_mut();
            if memo.len() >= LIMIT {
                memo.clear();
            }
            memo.insert((ty.node_addr(), version), (ty, resolved.clone()));
        });
    }
    resolved
}

/// Each class name `resolve_names_in_type` resolves in `ty`, as
/// `(written, resolved)`: `class.nameCase` needs the spelling the
/// resolution replaces.
pub(crate) fn collect_class_name_case_refs(
    ty: &Type,
    ctx: &NameContext,
    out: &mut Vec<(Name, Name)>,
) {
    ty.map_class_names(&mut |name| {
        let resolved = Name::from(ctx.resolve(name));
        out.push((Name::from(name), resolved.clone()));
        resolved
    });
}
