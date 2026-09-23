//! `[[parent_calls]]`: overriding methods that must (or must not) call
//! the method they override - checked by `Project::check_class_shapes`.
//!
//! ```toml
//! [[parent_calls]]
//! class = "App\\Http\\BaseController"   # any subclass; omit for all
//! method = "boot"
//! invocation = "first"                   # first | last | any | never
//! has_content = "yes"                    # yes | no | maybe (default)
//! ```
//!
//! Built-in entries cover PHPUnit's and Codeception's test hooks; like
//! every entry, they only apply when the overridden implementation isn't
//! empty.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Invocation {
    First,
    Last,
    Any,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HasContent {
    /// Besides the parent call, the method must do something.
    Yes,
    /// The method must consist of the parent call alone.
    No,
    #[default]
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParentCallRule {
    /// The class whose subclasses this applies to; any class when unset.
    pub class: Option<String>,
    pub method: String,
    pub invocation: Invocation,
    #[serde(default)]
    pub has_content: HasContent,
}

/// PHPUnit's and Codeception's lifecycle hooks.
pub fn default_rules() -> Vec<ParentCallRule> {
    let rule = |class: &str, method: &str, invocation| ParentCallRule {
        class: Some(class.to_string()),
        method: method.to_string(),
        invocation,
        has_content: HasContent::Maybe,
    };
    vec![
        rule("PHPUnit\\Framework\\TestCase", "setUp", Invocation::First),
        rule(
            "PHPUnit\\Framework\\TestCase",
            "setUpBeforeClass",
            Invocation::First,
        ),
        rule(
            "PHPUnit\\Framework\\TestCase",
            "assertPreConditions",
            Invocation::Any,
        ),
        rule(
            "PHPUnit\\Framework\\TestCase",
            "assertPostConditions",
            Invocation::Any,
        ),
        rule("PHPUnit\\Framework\\TestCase", "tearDown", Invocation::Last),
        rule(
            "PHPUnit\\Framework\\TestCase",
            "tearDownAfterClass",
            Invocation::Last,
        ),
        rule("Codeception\\Test\\Unit", "_before", Invocation::First),
        rule("Codeception\\Test\\Unit", "_after", Invocation::Last),
    ]
}
