use super::super::*;
use syntax::lexer::Lexer;
use syntax::parser::Parser;

/// The keys a file references, asked by name: whether any kind of
/// reference to the lowercase `name` is among them.
pub(super) struct Refs(FxHashSet<RefKey>);

impl Refs {
    pub(super) fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|key| key.name().lower() == name)
    }

    /// Whether a reference of `kind`'s own key to `name` is among them.
    pub(super) fn contains_key(&self, key: &RefKey) -> bool {
        self.0.contains(key)
    }
}

pub(super) fn refs(src: &str) -> Refs {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let ast = Parser::new(tokens).parse_program().unwrap();
    Refs(referenced_names(&ast))
}
