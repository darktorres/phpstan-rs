//! Writes the type-derived bound of every builtin function and method as TSV,
//! for the tracer replay in `tools/php-catalog/experiments/alternatives/tracer/replay.py`.
//!
//! Each line is one of:
//! - `M  class  method`: a method the builtin class declares;
//! - `P  key  at  name  callable`: a parameter, and whether its type takes a callable;
//! - `R  key  at  elements  kind  name`: a run `derived_runs` gives the parameter, where
//!   `kind` is `method` or `wrapper`.
//!
//! `key` is the lowercased function name, or `class::method`.
//!
//! Ignored by default. Run it with
//! `CATALOG_DERIVED_OUT=<file> cargo test -p phpstan-rs --lib writes_the_derived_bound -- --ignored`.

use super::*;
use crate::registry::stubs::{builtin_stub_classes, builtin_stubs};
use std::io::Write;

#[test]
#[ignore = "writes the derived bound to CATALOG_DERIVED_OUT for the tracer replay"]
fn writes_the_derived_bound() {
    let Some(path) = std::env::var_os("CATALOG_DERIVED_OUT") else {
        return;
    };
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).expect("creates the file"));
    for (name, sig) in builtin_stubs() {
        let name: &str = name.as_ref();
        write_signature(&mut out, &name.to_ascii_lowercase(), &sig);
    }
    for (class, reflection) in builtin_stub_classes() {
        let class: &str = class.as_ref();
        let class = class.to_ascii_lowercase();
        for (method, sig) in reflection.methods().iter() {
            let method: &str = method.as_ref();
            let method = method.to_ascii_lowercase();
            writeln!(out, "M\t{class}\t{method}").expect("writes");
            write_signature(&mut out, &format!("{class}::{method}"), sig);
        }
    }
    out.flush().expect("flushes");
}

fn write_signature(out: &mut impl Write, key: &str, sig: &FunctionReflection) {
    for (at, param) in sig.params().iter().enumerate() {
        let name: &str = param.name().as_ref();
        let callable = u8::from(param.ty().mentions_callable());
        writeln!(out, "P\t{key}\t{at}\t{name}\t{callable}").expect("writes");
    }
    for (position, run) in derived_runs(sig) {
        let (kind, name) = match run {
            BuiltinRun::Method(name) => ("method", name),
            BuiltinRun::Wrapper(name) => ("wrapper", name),
            BuiltinRun::Calls => continue,
        };
        let elements = u8::from(position.elements);
        writeln!(out, "R\t{key}\t{}\t{elements}\t{kind}\t{name}", position.at).expect("writes");
    }
}
