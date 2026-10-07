use crate::ast::*;
use crate::parse::{self, ParsedFile};

fn parsed(source: &str) -> ParsedFile {
    parse::parse(source.as_bytes()).expect("PHP parses")
}

fn functions(file: &ParsedFile) -> Vec<&FunctionDecl> {
    file.statements()
        .iter()
        .filter_map(|&id| {
            let stmt = file.arenas.stmt(id)?;
            let StmtKind::FunctionDecl(function) = &stmt.kind else {
                return None;
            };
            file.arenas.function(*function)
        })
        .collect()
}

fn function<'a>(file: &'a ParsedFile, name: &str) -> &'a FunctionDecl {
    functions(file)
        .into_iter()
        .find(|function| function.name.name == name)
        .expect("function is present")
}

fn param_types(file: &ParsedFile, function: &FunctionDecl) -> Vec<Option<String>> {
    file.arenas
        .params(function.params)
        .iter()
        .map(|param| {
            param
                .type_hint
                .as_ref()
                .map(|hint| hint.to_string_with(&file.arenas))
        })
        .collect()
}

fn doc_text<'a>(file: &'a ParsedFile, doc: &'a Option<DocCommentRanges>) -> Option<String> {
    doc.as_ref()
        .and_then(|ranges| file.doc_comment_text(ranges))
        .map(|text| text.into_owned())
}

#[test]
fn drops_closed_range_functions_and_methods_during_parse() {
    let file = parsed(
        "<?php #[PhpStormStubsElementAvailable(to: '7.4')] function old() {}\n\
         class C { #[PhpStormStubsElementAvailable(to: '7.4')] function old() {} }",
    );
    assert!(functions(&file).is_empty());
    let class = file.statements().iter().find_map(|&id| {
        let stmt = file.arenas.stmt(id)?;
        let StmtKind::ClassDecl(class) = &stmt.kind else {
            return None;
        };
        file.arenas.class(class.def)
    });
    let class = class.expect("class remains");
    assert!(file.arenas.methods(class.methods).is_empty());
    assert!(file.stub_attributes_changed);
    assert!(file.references.is_none());
}

#[test]
fn keeps_a_superseded_function_used_as_a_single_statement_body() {
    let file =
        parsed("<?php if (true) #[PhpStormStubsElementAvailable(to: '7.4')] function nested() {}");
    let root = file
        .arenas
        .stmt(file.statements()[0])
        .expect("if statement");
    let StmtKind::If(if_stmt) = &root.kind else {
        panic!("expected if statement")
    };
    let body = file
        .arenas
        .stmt(if_stmt.then_branch)
        .expect("single-statement body");
    let StmtKind::FunctionDecl(function_id) = &body.kind else {
        panic!("single-statement declarations keep their historical presence")
    };
    let function = file.arenas.function(*function_id).expect("nested function");
    assert!(file.arenas.attributes(function.attrs).is_empty());
    assert!(file.stub_attributes_changed);
}

#[test]
fn keeps_open_availability_ranges_and_removes_only_stub_attributes() {
    let file = parsed(
        "<?php #[Ordinary] #[PhpStormStubsElementAvailable(from: '8.0')] function current() {}",
    );
    let function = function(&file, "current");
    let attrs = file.arenas.attributes(function.attrs);
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name.written.as_str(), "Ordinary");
    assert!(file.stub_attributes_changed);
    assert!(file.references.is_none());
}

#[test]
fn filters_superseded_parameters_and_injects_the_newest_native_type() {
    let file = parsed(
        "<?php function f(#[PhpStormStubsElementAvailable(from: '5.3', to: '7.4')] $old, #[LanguageLevelTypeAware(['8.1' => 'int', '8.10' => 'string'], default: 'bool')] $value) {}",
    );
    let function = function(&file, "f");
    let params = file.arenas.params(function.params);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name.name.as_str(), "value");
    assert_eq!(param_types(&file, function), [Some("string".to_owned())]);
}

#[test]
fn type_aware_parameter_names_resolve_in_the_live_import_context() {
    let file = parsed(
        "<?php namespace App; use Vendor\\TypeName as Alias; function f(#[LanguageLevelTypeAware(['8.1' => 'Alias'], default: 'int')] $value) {}",
    );
    let function = function(&file, "f");
    let params = file.arenas.params(function.params);
    let Some(TypeHint {
        kind: TypeHintKind::Name(name),
        ..
    }) = params[0].type_hint.as_ref()
    else {
        panic!("injected hint is a class name")
    };
    assert_eq!(name.resolved_class(), Some("Vendor\\TypeName"));
    assert!(file.stub_attributes_changed);
}

#[test]
fn return_and_parameter_overrides_are_visible_in_final_doc_text() {
    let file = parsed(
        "<?php /** @param int $before */ function f(/** @param string $before */ $before, #[LanguageLevelTypeAware(['8.1' => 'bool'], default: 'int')] $value) {}\n\
         /** @return int */ #[LanguageLevelTypeAware(['8.1' => 'string'], default: 'int')] function g() {}",
    );
    let f = function(&file, "f");
    let f_doc = doc_text(&file, &f.doc_comment).expect("function doc remains");
    assert!(!f_doc.contains("@param bool $value"), "{f_doc}");
    let params = file.arenas.params(f.params);
    let before_doc = doc_text(&file, &params[0].doc_comment).expect("param doc remains");
    assert!(before_doc.contains("@param bool $value"), "{before_doc}");

    let g = function(&file, "g");
    let g_doc = doc_text(&file, &g.doc_comment).expect("return doc remains");
    assert!(g_doc.contains("@return string"), "{g_doc}");
}

#[test]
fn parameter_override_uses_function_doc_when_no_earlier_parameter_has_one() {
    let file = parsed(
        "<?php /** @param int $value */ function f(#[LanguageLevelTypeAware(['8.1' => 'bool'], default: 'int')] $value) {}",
    );
    let function = function(&file, "f");
    let text = doc_text(&file, &function.doc_comment).expect("function doc remains");
    assert!(text.contains("@param bool $value"), "{text}");
}

#[test]
fn type_aware_attributes_transform_class_methods_properties_and_constants() {
    let file = parsed(
        "<?php /** @return int */ #[LanguageLevelTypeAware(['8.1' => 'string'], default: 'int')] class C {\n\
         /** @return int */ #[LanguageLevelTypeAware(['8.1' => 'bool'], default: 'int')] function f() {}\n\
         /** @var int */ #[LanguageLevelTypeAware(['8.1' => 'float'], default: 'int')] public $property;\n\
         /** @var int */ #[LanguageLevelTypeAware(['8.1' => 'array'], default: 'int')] public const VALUE = 1;\n\
         }",
    );
    let class = file.statements().iter().find_map(|&id| {
        let stmt = file.arenas.stmt(id)?;
        let StmtKind::ClassDecl(class) = &stmt.kind else {
            return None;
        };
        file.arenas.class(class.def)
    });
    let class = class.expect("class remains");
    assert!(file.arenas.attributes(class.attrs).is_empty());
    let class_doc = doc_text(&file, &class.doc_comment).expect("class doc remains");
    assert!(class_doc.contains("@return string"), "{class_doc}");

    let method = &file.arenas.methods(class.methods)[0].function;
    let method_doc = doc_text(&file, &method.doc_comment).expect("method doc remains");
    assert!(method_doc.contains("@return bool"), "{method_doc}");
    let property = &class.properties(&file.arenas)[0];
    let property_doc = doc_text(&file, &property.doc_comment).expect("property doc remains");
    assert!(property_doc.contains("@return float"), "{property_doc}");
    let constant = &class.consts(&file.arenas)[0];
    let constant_doc = doc_text(&file, &constant.doc_comment).expect("constant doc remains");
    assert!(constant_doc.contains("@return array"), "{constant_doc}");
}

#[test]
fn resolves_attribute_aliases_and_does_not_match_a_short_name_without_an_import() {
    let aliased = parsed(
        "<?php use Vendor\\Internal\\PhpStormStubsElementAvailable as Gone; #[Gone(to: '7.4')] function old() {}",
    );
    assert!(functions(&aliased).is_empty());

    let unrelated = parsed("<?php function f(#[Gone(to: '7.4')] $value) {} ");
    let function = function(&unrelated, "f");
    let params = unrelated.arenas.params(function.params);
    assert_eq!(unrelated.arenas.attributes(params[0].attrs).len(), 1);
    assert!(!unrelated.stub_attributes_changed);
    assert!(unrelated.references.is_some());
}

#[test]
fn bracket_shorthand_stays_phpdoc_only_and_type_aware_attributes_are_removed() {
    let file = parsed(
        "<?php function f(#[LanguageLevelTypeAware(['8.1' => 'string[]'], default: 'string')] $value = null) {}",
    );
    let function = function(&file, "f");
    let params = file.arenas.params(function.params);
    assert!(params[0].type_hint.is_none());
    assert!(file.arenas.attributes(params[0].attrs).is_empty());
}
