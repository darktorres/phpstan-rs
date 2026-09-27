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

/// `[rules.class_shape]`: which classes are closed, and which methods an
/// override must call its parent's version of.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ClassShapeSettings {
    /// Classes closed by default (`project::class_shape`).
    pub check_closed_classes: bool,
    pub parent_calls: Vec<ParentCallRule>,
    /// Whether the built-in PHPUnit/Codeception hook requirements
    /// (`default_rules`) apply.
    pub default_parent_calls: bool,
}

impl Default for ClassShapeSettings {
    fn default() -> Self {
        ClassShapeSettings {
            check_closed_classes: false,
            parent_calls: Vec::new(),
            default_parent_calls: true,
        }
    }
}

impl config::Section for ClassShapeSettings {
    const PATH: &'static [&'static str] = &["rules", "class_shape"];
}

impl ClassShapeSettings {
    /// The configured requirements, then the built-in ones unless turned
    /// off.
    pub fn rules(&self) -> Vec<ParentCallRule> {
        let mut rules = self.parent_calls.clone();
        if self.default_parent_calls {
            rules.extend(default_rules());
        }
        rules
    }
}

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
