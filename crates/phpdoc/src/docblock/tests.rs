mod api_tags;
mod assert_tags;
mod callable_invocation_tags;
mod method_and_mixin_tags;
mod param_and_return_tags;
mod psalm_and_phan_tags;
mod templates_and_generics;
mod type_aliases;
mod unknown_tags;

/// The type a type written as `source` parses to.
fn node(source: &str) -> crate::DocType {
    crate::grammar::parse_type_prefix(source)
        .0
        .expect("a type")
        .into()
}
