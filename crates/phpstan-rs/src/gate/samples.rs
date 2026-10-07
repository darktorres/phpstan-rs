//! Values of a declared type, for the runtime oracle to call a fixture's
//! functions with: each a PHP expression with the type of the value it
//! makes, which the declared type surely accepts. Scalars come from a pool
//! of literals, filtered by the relation; arrays, objects, enum cases,
//! class strings and closures are built from the type itself.

use types::{PhpString, Shape, ShapeField, ShapeKey, Type};

use crate::registry::Registry;

/// A PHP expression and the type of the value it makes.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub php: String,
    pub ty: Type,
}

/// The most values given for one type.
const LIMIT: usize = 6;

/// How deep arrays, shapes and constructor arguments nest.
const DEPTH: usize = 2;

/// Makes values of the types a registry resolves.
pub struct Sampler<'r> {
    registry: &'r Registry,
}

impl<'r> Sampler<'r> {
    pub fn new(registry: &'r Registry) -> Sampler<'r> {
        Sampler { registry }
    }

    /// Up to `LIMIT` values `ty` surely accepts, the kinds of value apart
    /// first; none when no value of it can be made.
    pub fn samples(&self, ty: &Type) -> Vec<Sample> {
        self.samples_at(ty, DEPTH)
    }

    fn samples_at(&self, ty: &Type, depth: usize) -> Vec<Sample> {
        let ty = match ty.template_bound() {
            Some(bound) if ty.is_template() => bound.clone(),
            _ => ty.clone(),
        };
        // An object, enum case, class string or closure built from a member
        // of `ty` is of it by construction (an object of a generic class is
        // of every instantiation the class has at run time); an array, and
        // what the pool offers, only when the relation says so.
        let relation = self.registry.relation();
        let mut out: Vec<Sample> = Vec::new();
        let mut offer = |candidate: Sample, checked: bool| {
            if !out.iter().any(|s| s.php == candidate.php)
                && (!checked || relation.is_super_type_of(&ty, &candidate.ty).yes())
            {
                out.push(candidate);
            }
        };
        for member in ty.union_members() {
            for candidate in self.built(member, depth) {
                offer(candidate, false);
            }
            if member.is_array().yes() && depth > 0 {
                for candidate in self.arrays(member, depth) {
                    offer(candidate, true);
                }
            }
        }
        for candidate in self.pool(&ty) {
            offer(candidate, true);
        }
        spread(out)
    }

    /// The values built from `ty`'s own shape that are of it by
    /// construction: objects, enum cases, class strings, closures.
    fn built(&self, ty: &Type, depth: usize) -> Vec<Sample> {
        let mut out = Vec::new();
        if let Some(case) = ty.as_enum_case() {
            out.push(enum_case(case.0, case.1));
            return out;
        }
        if let Some(shape) = ty.as_object_shape() {
            out.extend(self.object_shape(shape, depth));
            return out;
        }
        if let Some(class) = ty.class_string_class() {
            for concrete in self.concrete(class) {
                out.push(Sample {
                    php: format!("\\{concrete}::class"),
                    ty: Type::class_string(Type::object(concrete.as_str().into(), Vec::new())),
                });
            }
        }
        if ty.is_object().yes() || ty.is_mixed() {
            out.extend(self.objects(ty, depth));
        }
        // A closure cannot answer for a callable with templates of its
        // own: what it returns depends on what it is given.
        let generic = ty
            .callable_sig()
            .is_some_and(|sig| !sig.templates.is_empty());
        if (ty.callable_sig().is_some() || ty.is_mixed()) && !generic {
            let returned = ty
                .callable_sig()
                .map(|sig| self.samples_at(&sig.return_type, depth.saturating_sub(1)))
                .and_then(|samples| samples.into_iter().next())
                .map_or_else(|| "null".to_string(), |sample| sample.php);
            // A closure is accepted by any callable it may be called as.
            out.push(Sample {
                php: format!("static function (...$__a) {{ return {returned}; }}"),
                ty: ty.clone(),
            });
        }
        out
    }

    /// The objects of the classes `ty` names: each enum's cases, and an
    /// instance of each class that can have one, a named abstract class
    /// or interface standing for its concrete descendants.
    fn objects(&self, ty: &Type, depth: usize) -> Vec<Sample> {
        let mut out = Vec::new();
        let classes = ty.object_class_names();
        if classes.is_empty() {
            out.push(Sample {
                php: "new \\stdClass()".to_string(),
                ty: Type::object("stdClass".into(), Vec::new()),
            });
        }
        for class in classes {
            for concrete in self.concrete(class) {
                let Some((name, info)) = self.registry.lookup_class(&concrete) else {
                    continue;
                };
                if info.kind() == syntax::ast::ClassKind::Enum {
                    for case in info.enum_case_values().keys() {
                        out.push(enum_case(name, case));
                    }
                    continue;
                }
                out.push(Sample {
                    php: self.instance(name, depth),
                    ty: Type::object(name.into(), Vec::new()),
                });
            }
        }
        out
    }

    /// An object with each property of `shape` (and without each optional
    /// one), cast from an array.
    fn object_shape(&self, shape: &Shape, depth: usize) -> Vec<Sample> {
        let mut required = Vec::new();
        let mut all = Vec::new();
        for field in shape.fields() {
            let value = (depth > 0)
                .then(|| self.samples_at(&field.ty, depth - 1))
                .and_then(|samples| samples.into_iter().next());
            let Some(value) = value else {
                if field.optional {
                    continue;
                }
                return Vec::new();
            };
            all.push((field.key.clone(), value.clone()));
            if !field.optional {
                required.push((field.key.clone(), value));
            }
        }
        let object = |fields: Vec<(ShapeKey, Sample)>| {
            let array = array(fields);
            Sample {
                php: format!("(object) {}", array.php),
                ty: Type::object("stdClass".into(), Vec::new()),
            }
        };
        let mut out = vec![object(all.clone())];
        if required.len() < all.len() {
            out.push(object(required));
        }
        out
    }

    /// `class` itself when it can be instantiated or is an enum, else its
    /// concrete descendants.
    fn concrete(&self, class: &str) -> Vec<String> {
        let Some((name, info)) = self.registry.lookup_class(class) else {
            return Vec::new();
        };
        let instantiable = |info: &crate::registry::ClassReflection| {
            matches!(
                info.kind(),
                syntax::ast::ClassKind::Class | syntax::ast::ClassKind::Enum
            ) && !info.is_abstract()
        };
        if instantiable(info) {
            return vec![name.to_string()];
        }
        let mut out: Vec<String> = self
            .registry
            .subclasses(name)
            .into_iter()
            .filter(|sub| {
                self.registry
                    .lookup_class(sub)
                    .is_some_and(|(_, info)| instantiable(info))
            })
            .map(|sub| sub.to_string())
            .collect();
        out.sort();
        out.truncate(2);
        out
    }

    /// An expression making an instance of `class`: through its
    /// constructor when every parameter it needs has a value, else without
    /// running it.
    fn instance(&self, class: &str, depth: usize) -> String {
        let constructor = self.registry.lookup_method(class, "__construct");
        let args: Option<Vec<String>> = match constructor {
            None => Some(Vec::new()),
            Some(sig) => sig
                .params()
                .iter()
                .filter(|p| !p.has_default() && !p.variadic())
                .map(|p| {
                    (depth > 0 && !p.by_ref())
                        .then(|| self.samples_at(p.ty(), depth - 1))
                        .and_then(|samples| samples.into_iter().next())
                        .map(|sample| sample.php)
                })
                .collect(),
        };
        match args {
            Some(args) => format!("new \\{class}({})", args.join(", ")),
            None => format!(
                "(new \\ReflectionClass(\\{class}::class))->newInstanceWithoutConstructor()"
            ),
        }
    }

    /// The arrays of `ty`: a shape with each field's value, and without
    /// each optional field; else an empty one, and one and two elements
    /// of its key and value types.
    fn arrays(&self, ty: &Type, depth: usize) -> Vec<Sample> {
        let mut out = Vec::new();
        if let Some(shape) = ty.as_array_shape() {
            let mut fields = Vec::new();
            for field in shape.fields() {
                let Some(value) = self.samples_at(&field.ty, depth - 1).into_iter().next() else {
                    if field.optional {
                        continue;
                    }
                    return out;
                };
                fields.push((field.key.clone(), value, field.optional));
            }
            out.push(array(
                fields
                    .iter()
                    .map(|(k, v, _)| (k.clone(), v.clone()))
                    .collect(),
            ));
            if fields.iter().any(|(_, _, optional)| *optional) {
                out.push(array(
                    fields
                        .iter()
                        .filter(|(_, _, optional)| !optional)
                        .map(|(k, v, _)| (k.clone(), v.clone()))
                        .collect(),
                ));
            }
            return out;
        }
        out.push(array(Vec::new()));
        let values = self.samples_at(
            &ty.array_value_type().unwrap_or_else(Type::mixed),
            depth - 1,
        );
        if values.is_empty() {
            return out;
        }
        let list = ty.is_list().yes();
        let keys: Vec<ShapeKey> = if list {
            vec![ShapeKey::Int(0), ShapeKey::Int(1)]
        } else {
            self.samples_at(&ty.array_key_type().unwrap_or_else(Type::mixed), depth - 1)
                .into_iter()
                .filter_map(|key| ShapeKey::from_type(&key.ty))
                .take(2)
                .collect()
        };
        for count in 1..=keys.len() {
            out.push(array(
                keys.iter()
                    .take(count)
                    .enumerate()
                    .map(|(i, key)| (key.clone(), values[i % values.len()].clone()))
                    .collect(),
            ));
        }
        out
    }

    /// The literals of a pool, with the bounds of an int range and the
    /// strings `ty` names.
    fn pool(&self, ty: &Type) -> Vec<Sample> {
        let mut out = Vec::new();
        let int = |n: i64| Sample {
            php: if n == i64::MIN {
                "PHP_INT_MIN".to_string()
            } else {
                n.to_string()
            },
            ty: Type::int_literal(n),
        };
        if let Some((min, max)) = ty.int_bounds() {
            for n in [min, max].into_iter().flatten() {
                out.push(int(n));
            }
            if let (Some(min), Some(max)) = (min, max) {
                let middle = (i128::from(min) + i128::from(max)) / 2;
                out.push(int(middle as i64));
            }
        }
        for n in [0, 1, -1, 2, 7, 42, i64::MAX, i64::MIN] {
            out.push(int(n));
        }
        for f in [0.0, 1.5, -2.5] {
            out.push(Sample {
                php: format!("{f:?}"),
                ty: Type::float_literal(f),
            });
        }
        for text in ty.constant_strings() {
            out.push(string(text.as_bytes()));
        }
        // One of each kind first: empty, numeric, a word, a class's name, a
        // function's, a float's.
        for text in [
            "", "0", "a", "stdClass", "strlen", "1.5", "1", "abc", "A", " ",
        ] {
            out.push(string(text.as_bytes()));
        }
        out.push(Sample {
            php: "true".to_string(),
            ty: Type::bool_literal(true),
        });
        out.push(Sample {
            php: "false".to_string(),
            ty: Type::bool_literal(false),
        });
        out.push(Sample {
            php: "null".to_string(),
            ty: Type::null(),
        });
        if ty.is_resource(false).yes() || ty.is_mixed() {
            out.push(Sample {
                php: "fopen('php://memory', 'r')".to_string(),
                ty: Type::resource(false),
            });
        }
        out
    }
}

/// `values` with the kinds of value apart first, up to `LIMIT`.
fn spread(values: Vec<Sample>) -> Vec<Sample> {
    let kind = |sample: &Sample| -> u8 {
        let ty = &sample.ty;
        if ty.is_null().yes() {
            0
        } else if ty.is_bool().yes() {
            1
        } else if ty.is_int().yes() {
            2
        } else if ty.is_float().yes() {
            3
        } else if ty.is_string().yes() {
            4
        } else if ty.is_array().yes() {
            5
        } else {
            6
        }
    };
    let mut first: Vec<Sample> = Vec::new();
    let mut rest: Vec<Sample> = Vec::new();
    for value in values {
        if first.iter().any(|s| kind(s) == kind(&value)) {
            rest.push(value);
        } else {
            first.push(value);
        }
    }
    first.extend(rest);
    first.truncate(LIMIT);
    first
}

fn enum_case(class: &str, case: &str) -> Sample {
    Sample {
        php: format!("\\{class}::{case}"),
        ty: Type::enum_case(class.into(), case.into()),
    }
}

fn string(bytes: &[u8]) -> Sample {
    let mut php = String::from("\"");
    for &b in bytes {
        match b {
            b'"' | b'\\' | b'$' => {
                php.push('\\');
                php.push(b as char);
            }
            0x20..=0x7e => php.push(b as char),
            _ => php.push_str(&format!("\\x{b:02x}")),
        }
    }
    php.push('"');
    Sample {
        php,
        ty: Type::string_literal(PhpString::from_bytes(bytes)),
    }
}

/// An array literal of `fields`, typed as the shape PHP makes of it.
fn array(fields: Vec<(ShapeKey, Sample)>) -> Sample {
    let php = fields
        .iter()
        .map(|(key, value)| {
            let key = match key {
                ShapeKey::Int(n) => n.to_string(),
                ShapeKey::Str(s) => string(s.as_bytes()).php,
            };
            format!("{key} => {}", value.php)
        })
        .collect::<Vec<_>>()
        .join(", ");
    Sample {
        php: format!("[{php}]"),
        ty: Type::array_shape(Shape::sealed(
            fields
                .into_iter()
                .map(|(key, value)| ShapeField::new(key, value.ty, false))
                .collect(),
        )),
    }
}

/// The most calls made of one function.
const CALLS: usize = 6;

/// PHP that calls each function and method declared at the top of the
/// `files` (each a path and its source) whose body asserts something, with
/// values of its parameters' types: one call for each value of the
/// parameter with the most, the others' values cycled. A parameter with a
/// default is left out from the first one with no value; a function with a
/// parameter needing one and none to give is not called. Each call is
/// made apart, so that one that throws leaves the others.
pub fn runtime_calls(registry: &Registry, files: &[(&str, &str)]) -> String {
    let sampler = Sampler::new(registry);
    let mut php = String::new();
    for (_, source) in files {
        let Ok(parsed) = syntax::parse(source.as_bytes()) else {
            continue;
        };
        let mut found = Declared::default();
        syntax::visit::walk_stmts(&mut found, &parsed.stmts);
        let asserts = |span: syntax::span::Span| {
            source
                .get(span.start as usize..span.end as usize)
                .is_some_and(|text| text.contains("assert"))
        };
        for function in &found.functions {
            if !asserts(function.span) {
                continue;
            }
            let name = function.fq_name().trim_start_matches('\\');
            let Some(sig) = registry.lookup_function(name) else {
                continue;
            };
            php.push_str(&calls(&sampler, sig, &|args| format!("\\{name}({args})")));
        }
        for (class, method) in &found.methods {
            if method.is_abstract || !asserts(method.function.span) {
                continue;
            }
            let name = &method.function.name;
            let Some(sig) = registry.lookup_method(class, name) else {
                continue;
            };
            let receiver = if method.is_static {
                None
            } else {
                let Some(object) = sampler
                    .samples(&Type::object(class.as_str().into(), Vec::new()))
                    .into_iter()
                    .next()
                else {
                    continue;
                };
                Some(object.php)
            };
            let call = |args: &str| match (&receiver, method.visibility) {
                (Some(object), syntax::ast::Visibility::Public) => {
                    format!("({object})->{name}({args})")
                }
                (None, syntax::ast::Visibility::Public) => format!("\\{class}::{name}({args})"),
                // A method the class alone may call, called from its scope.
                (Some(object), _) => format!(
                    "(function (...$__a) {{ return $this->{name}(...$__a); }})->call({object}, ...[{args}])"
                ),
                (None, _) => format!(
                    "\\Closure::bind(static function (...$__a) {{ return static::{name}(...$__a); }}, null, \\{class}::class)({args})"
                ),
            };
            php.push_str(&calls(&sampler, sig, &call));
        }
    }
    php
}

/// The calls of `sig`, each written by `call` from its arguments.
fn calls(
    sampler: &Sampler,
    sig: &crate::registry::FunctionReflection,
    call: &dyn Fn(&str) -> String,
) -> String {
    let mut values: Vec<(Vec<Sample>, bool)> = Vec::new();
    for param in sig.params() {
        if param.variadic() {
            break;
        }
        let samples = sampler.samples(param.ty());
        if samples.is_empty() {
            if param.has_default() {
                break;
            }
            return String::new();
        }
        values.push((samples, param.by_ref()));
    }
    let count = values
        .iter()
        .map(|(samples, _)| samples.len())
        .max()
        .unwrap_or(1)
        .min(CALLS);
    let mut php = String::new();
    for k in 0..count {
        let mut prelude = String::new();
        let mut args = Vec::new();
        for (i, (samples, by_ref)) in values.iter().enumerate() {
            let value = &samples[k % samples.len()].php;
            if *by_ref {
                prelude.push_str(&format!("$__r{i} = {value}; "));
                args.push(format!("$__r{i}"));
            } else {
                args.push(value.clone());
            }
        }
        php.push_str(&format!(
            "try {{ {prelude}{}; }} catch (\\Throwable $__e) {{}}\n",
            call(&args.join(", "))
        ));
    }
    php
}

/// The functions and the methods of the classes a file declares, outside
/// any function's body.
#[derive(Default)]
struct Declared<'ast> {
    functions: Vec<&'ast syntax::ast::FunctionDecl>,
    methods: Vec<(String, &'ast syntax::ast::MethodDecl)>,
}

impl<'ast> syntax::visit::Visitor<'ast> for Declared<'ast> {
    fn visit_stmt(&mut self, stmt: &'ast syntax::ast::StmtNode) {
        match &stmt.kind {
            syntax::ast::StmtKind::FunctionDecl(function) => self.functions.push(function),
            syntax::ast::StmtKind::ClassDecl(class) => {
                if !matches!(
                    class.def.kind,
                    syntax::ast::ClassKind::Interface | syntax::ast::ClassKind::Trait
                ) {
                    let name = class.fq_name().trim_start_matches('\\').to_string();
                    for method in &class.def.methods {
                        self.methods.push((name.clone(), method));
                    }
                }
            }
            _ => syntax::visit::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, _: &'ast syntax::ast::ExprNode) {}
}

#[cfg(test)]
mod tests;
