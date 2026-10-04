//! The canonical prints (`syntax::canonical`) of a doc comment's parse and
//! of the `DocBlock` it reduces to. A `DocType` prints as its node, the
//! `Type` it denotes as written and whether it spells out `mixed`.

use syntax::canonical::{Canonical, Printer};
use syntax::{canonical_enum, canonical_struct};

use crate::DocType;
use crate::docblock::{
    AssertTag, DocBlock, InvalidTagValue, MethodTag, PropertyTag, TypeImportTag,
};
use crate::grammar::{
    ArgVariance, ArrayShapeField, CallableParam, CallableTemplate, MethodTagParam, PhpDocNode,
    PhpDocParseError, PhpDocTag, PhpDocTagValue, ShapeKey, TypeNode,
};

impl Canonical for DocType {
    fn canonical(&self, p: &mut Printer) {
        p.tuple("DocType", &[self.node(), self.ty(), &self.mentions_mixed()]);
    }
}

canonical_enum!(TypeNode {
    Identifier(name),
    Nullable(inner),
    Union(members),
    Intersection(members),
    Generic(name, args),
    Wildcard,
    VarianceArg(variance, inner),
    ArrayOf(inner),
    OffsetAccess(base, offset),
    ArrayShape {
        keyword,
        fields,
        sealed,
        unsealed_type,
    },
    Callable {
        keyword,
        templates,
        params,
        return_type,
    },
    IntRange(min, max),
    IntLiteral(value),
    FloatLiteral(value),
    StringLiteral(value),
    ConstFetch { class, const_name },
    Conditional {
        subject,
        negated,
        target,
        if_true,
        if_false,
    },
    Paren(inner),
    Error,
});

canonical_enum!(ArgVariance {
    Covariant,
    Contravariant,
});

canonical_struct!(ArrayShapeField { key, ty, optional });

canonical_enum!(ShapeKey {
    Int(value),
    String(value),
    Identifier(name),
});

canonical_struct!(CallableTemplate {
    name,
    bound,
    lower,
    default,
});

canonical_struct!(CallableParam {
    ty,
    variadic,
    by_ref,
    optional,
});

canonical_struct!(MethodTagParam {
    ty,
    name,
    variadic,
    by_ref,
    has_default,
});

canonical_struct!(PhpDocNode { tags, errors });

canonical_struct!(PhpDocParseError { message, span });

canonical_struct!(PhpDocTag {
    name,
    span,
    value,
    errors,
});

canonical_enum!(PhpDocTagValue {
    TypedVar {
        ty,
        variable,
        by_ref,
        variadic,
        description,
    },
    Type { ty, description },
    Template {
        name,
        bound,
        default,
    },
    ExtendsLike { ty },
    Method {
        is_static,
        return_type,
        name,
        templates,
        params,
    },
    Generic(text),
});

canonical_struct!(DocBlock {
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
    always_read,
    always_written,
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
});

canonical_struct!(InvalidTagValue {
    tag,
    value,
    message,
});

canonical_struct!(TypeImportTag {
    alias,
    from,
    as_name,
});

canonical_struct!(MethodTag {
    is_static,
    return_type,
    name,
    templates,
    params,
});

canonical_struct!(AssertTag {
    negated,
    equality,
    ty,
    target,
});

canonical_struct!(PropertyTag {
    name,
    ty,
    readable,
    writable,
});
