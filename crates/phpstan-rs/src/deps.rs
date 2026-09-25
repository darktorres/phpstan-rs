use rustc_hash::FxHashSet;

use syntax::ast::*;
use syntax::visit::{
    Visitor, walk_attribute, walk_catch_clause, walk_class_decl, walk_enum_decl, walk_expr,
    walk_stmts,
};

/// Best-effort, deliberately over-approximating set of every function/class
/// name one file's parse tree could reference externally - used purely to
/// decide which *other* files might need re-checking when a symbol they
/// declare changes. Missing a real reference here would let a stale
/// diagnostic survive an edit elsewhere; collecting an extra name that
/// turns out unrelated just costs one harmless re-check, so this errs
/// toward over-collecting rather than trying to be precise.
///
/// Names are kept exactly as the AST spells them (no backslash-stripping,
/// no namespace/`use`-import resolution) - a deliberate, still-safe
/// simplification even though the registry itself is namespace-aware
/// (`name_resolution::NameContext`): this is only ever used to decide
/// which files *might* need re-checking, never to look anything up in the
/// registry directly, so a same-named-but-different-namespace false
/// match just costs one harmless extra re-check (this function already
/// over-approximates on purpose, per the doc above) rather than a wrong
/// diagnostic. Type hints are the one exception: they're strings that get
/// resolved through `types::parse_type_string`, which does strip a
/// leading backslash, so the names pulled out of them are stripped the
/// same way to stay consistent with that.
pub fn referenced_names(stmts: &[Stmt]) -> FxHashSet<String> {
    let mut collector = Collector {
        names: FxHashSet::default(),
    };
    walk_stmts(&mut collector, stmts);
    collector.names
}

/// Collects every name a file mentions: class-shaped references in
/// expressions, declarations, catch clauses, attributes and type
/// declarations, anywhere in the file (nested scopes included).
struct Collector {
    names: FxHashSet<String>,
}

impl<'ast> Visitor<'ast> for Collector {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        match &expr.kind {
            ExprKind::ClassConstFetch(StaticTarget::Name(class, _), _)
            | ExprKind::ClassNameFetch(StaticTarget::Name(class, _))
            | ExprKind::StaticPropertyFetch(StaticTarget::Name(class, _), _)
            | ExprKind::DynamicStaticPropertyFetch(StaticTarget::Name(class, _), _)
            | ExprKind::DynamicClassConstFetch(StaticTarget::Name(class, _), _)
            | ExprKind::StaticCall(StaticTarget::Name(class, _), _, _)
            | ExprKind::DynamicStaticCall(StaticTarget::Name(class, _), _, _)
            | ExprKind::Name(class)
            | ExprKind::Instanceof(_, InstanceofTarget::Name(class, _)) => {
                self.names.insert(class.written.to_string());
            }
            _ => {}
        }
        walk_expr(self, expr);
    }

    fn visit_class_decl(&mut self, class: &'ast ClassDecl) {
        self.names
            .extend(class.extends.iter().map(|n| n.written.to_string()));
        self.names
            .extend(class.implements.iter().map(|n| n.written.to_string()));
        self.names
            .extend(class.uses.iter().map(|n| n.written.to_string()));
        walk_class_decl(self, class);
    }

    fn visit_enum_decl(&mut self, decl: &'ast EnumDecl) {
        self.names
            .extend(decl.implements.iter().map(|n| n.written.to_string()));
        self.names
            .extend(decl.uses.iter().map(|n| n.written.to_string()));
        walk_enum_decl(self, decl);
    }

    fn visit_catch_clause(&mut self, clause: &'ast CatchClause) {
        self.names
            .extend(clause.types.iter().map(|n| n.written.to_string()));
        walk_catch_clause(self, clause);
    }

    /// A class used only as an attribute (`#[MyAttribute]` on some other
    /// declaration) has no other node pointing at it, so without this it
    /// would look identically unreferenced to a genuinely dead class.
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.names.insert(attribute.name.written.to_string());
        walk_attribute(self, attribute);
    }

    /// Type declarations name classes without the leading `\`, the way
    /// type resolution spells them.
    fn visit_type_hint(&mut self, hint: &'ast TypeHint) {
        for (name, _) in hint.class_names() {
            self.names.insert(name.trim_start_matches('\\').to_string());
        }
    }
}

#[cfg(test)]
mod tests;
