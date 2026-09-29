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
    ty.map_class_names(&mut |name| ctx.resolve(name).into())
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
