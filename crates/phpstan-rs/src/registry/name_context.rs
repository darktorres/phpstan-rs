use std::sync::Arc;

use syntax::intern::intern;
use syntax::name_resolution::NameContext;
use types::Type;

/// `ty` with every class name resolved as `ctx` resolves it.
pub(crate) fn resolve_names_in_type(ty: Type, ctx: &NameContext) -> Type {
    ty.map_class_names(&mut |name| resolve_class_name(name, ctx))
}

/// `name` as `ctx` resolves it. `Traversable` stays as it is: it is
/// also the synthetic `iterable` desugars to, never a name to resolve,
/// though a written `Traversable<Edge>`'s arguments still are.
fn resolve_class_name(name: &types::Name, ctx: &NameContext) -> types::Name {
    if name == "Traversable" {
        name.clone()
    } else {
        ctx.resolve(name).into()
    }
}

/// Each class name `resolve_names_in_type` resolves in `ty`, as
/// `(written, resolved)`: `class.nameCase` needs the spelling the
/// resolution replaces.
pub(crate) fn collect_class_name_case_refs(
    ty: &Type,
    ctx: &NameContext,
    out: &mut Vec<(Arc<str>, Arc<str>)>,
) {
    ty.map_class_names(&mut |name| {
        let resolved = resolve_class_name(name, ctx);
        if name != "Traversable" {
            out.push((intern(name), intern(&resolved)));
        }
        resolved
    });
}
