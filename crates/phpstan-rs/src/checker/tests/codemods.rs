//! Suggested fixes, checked by applying them to the source.

use super::helpers::*;
use diagnostics::DiagnosticFix;

fn fixed(
    src: &str,
    identifier: &str,
    configure: impl FnOnce(&mut crate::registry::Registry),
) -> String {
    let d = all_diagnostics_for_with(src, configure);
    let fixes: Vec<&DiagnosticFix> = d
        .iter()
        .filter(|x| x.identifier == identifier)
        .filter_map(|x| x.fix.as_ref())
        .collect();
    assert!(!fixes.is_empty(), "no {identifier} fix in {d:?}");
    diagnostics::fix::apply_fixes(src, fixes).0
}

#[test]
fn function_and_method_name_case_are_fixed() {
    let src = "<?php\nfunction doThing(): void {}\nclass A { public function run(): void {} }\nfunction g(A $a): void {\n    DOTHING();\n    $a->RUN();\n}\n";
    let enable = |r: &mut crate::registry::Registry| r.check_function_name_case = true;
    assert_eq!(
        fixed(src, "function.nameCase", enable),
        src.replace("DOTHING()", "doThing()")
    );
    assert_eq!(
        fixed(src, "method.nameCase", enable),
        src.replace("$a->RUN()", "$a->run()")
    );
}

#[test]
fn a_used_function_import_is_fixed_in_place() {
    let src = "<?php\nnamespace Lib { function helper(): void {} }\nnamespace App {\nuse function lib\\HELPER;\n}\n";
    let enable = |r: &mut crate::registry::Registry| r.check_function_name_case = true;
    assert_eq!(
        fixed(src, "function.nameCase", enable),
        src.replace("lib\\HELPER", "Lib\\helper")
    );
}

#[test]
fn unused_private_members_are_deleted_with_their_doc_comments() {
    let src = "<?php\nfinal class A {\n    /** Never used. */\n    private const UNUSED = 1;\n    private const A = 1, B = 2;\n\n    #[\\Deprecated]\n    private ?int $gone;\n\n    /**\n     * Helper.\n     */\n    private function helper(): void {}\n\n    public function used(): int { return self::A; }\n}\n";
    let d = all_diagnostics_for(src);
    let fixes: Vec<&DiagnosticFix> = d
        .iter()
        .filter(|x| x.identifier.ends_with(".unused"))
        .filter_map(|x| x.fix.as_ref())
        .collect();
    let (out, applied) = diagnostics::fix::apply_fixes(src, fixes);
    assert_eq!(applied, 3, "{d:?}");
    assert_eq!(
        out,
        "<?php\nfinal class A {\n    private const A = 1, B = 2;\n\n\n\n    public function used(): int { return self::A; }\n}\n"
    );
    assert!(
        d.iter()
            .any(|x| x.message.contains("A::B is unused") && x.fix.is_none()),
        "a constant sharing its declaration has no fix: {d:?}"
    );
}

#[test]
fn a_missing_parent_constructor_call_is_inserted_when_it_needs_no_arguments() {
    let enable = |r: &mut crate::registry::Registry| r.check_parent_constructor_call = true;
    let src = "<?php\nclass P { public function __construct(int $x = 0) {} }\nclass C extends P {\n    public function __construct() {\n        $this->ready();\n    }\n    private function ready(): void {}\n}\nclass E extends P {\n    public function __construct() {}\n}\n";
    let out = fixed(src, "constructor.missingParentCall", enable);
    assert_eq!(
        out,
        "<?php\nclass P { public function __construct(int $x = 0) {} }\nclass C extends P {\n    public function __construct() {\n        parent::__construct();\n        $this->ready();\n    }\n    private function ready(): void {}\n}\nclass E extends P {\n    public function __construct() {\n        parent::__construct();\n    }\n}\n"
    );

    let required = "<?php\nclass P { public function __construct(int $x) {} }\nclass C extends P {\n    public function __construct() {}\n}\n";
    let d = all_diagnostics_for_with(required, enable);
    let finding = d
        .iter()
        .find(|x| x.identifier == "constructor.missingParentCall")
        .expect("reported");
    assert!(finding.fix.is_none(), "no fix when arguments are required");
}
