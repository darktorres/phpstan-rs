//! Tables for libraries whose narrowing the declarations themselves
//! don't state:
//!
//! - `[[assertions]]`: a function or method that asserts a parameter's
//!   type, as if its declaration carried the `@phpstan-assert` tag;
//! - `[[class_argument_returns]]`: a method returning an instance of the
//!   class its argument names (`Mockery::mock(Foo::class)`), optionally
//!   intersected with more types.
//!
//! ```toml
//! [[assertions]]
//! class = "My\\Assert"      # or `function = "my_assert"`
//! method = "isUser"
//! parameter = "value"
//! type = "App\\User"        # `!` prefix: asserts it's NOT that type
//! when = "always"           # always | true | false (the return value)
//!
//! [[class_argument_returns]]
//! class = "My\\Locator"
//! method = "get"
//! argument = 1              # 1-based
//! intersect = ["My\\Proxy"]
//! ```
//!
//! Built-in entries cover Nette Tester's `Assert` and Mockery's mocks and
//! spies. A container's `get(Foo::class)` is left to configuration: PSR-11
//! promises nothing about what an id resolves to.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AssertWhen {
    #[default]
    Always,
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionEntry {
    pub class: Option<String>,
    pub function: Option<String>,
    pub method: Option<String>,
    pub parameter: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub when: AssertWhen,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassArgumentReturn {
    pub class: String,
    pub method: String,
    pub argument: usize,
    #[serde(default)]
    pub intersect: Vec<String>,
}

impl AssertionEntry {
    pub fn validate(&self) -> Result<(), String> {
        match (&self.function, &self.class, &self.method) {
            (Some(_), None, None) | (None, Some(_), Some(_)) => Ok(()),
            _ => Err(format!(
                "an [[assertions]] entry needs either `function`, or `class` and `method` (parameter {})",
                self.parameter
            )),
        }
    }
}

impl ClassArgumentReturn {
    pub fn validate(&self) -> Result<(), String> {
        if self.argument == 0 {
            return Err(format!(
                "[[class_argument_returns]] {}::{}: `argument` is 1-based",
                self.class, self.method
            ));
        }
        Ok(())
    }
}

fn tester(method: &str, parameter: &str, ty: &str) -> AssertionEntry {
    AssertionEntry {
        class: Some("Tester\\Assert".to_string()),
        function: None,
        method: Some(method.to_string()),
        parameter: parameter.to_string(),
        ty: ty.to_string(),
        when: AssertWhen::Always,
    }
}

pub fn default_assertions() -> Vec<AssertionEntry> {
    vec![
        tester("true", "actual", "true"),
        tester("false", "actual", "false"),
        tester("null", "actual", "null"),
        tester("notNull", "actual", "!null"),
        tester("nan", "actual", "float"),
    ]
}

pub fn default_class_argument_returns() -> Vec<ClassArgumentReturn> {
    let entry =
        |class: &str, method: &str, argument: usize, intersect: &[&str]| ClassArgumentReturn {
            class: class.to_string(),
            method: method.to_string(),
            argument,
            intersect: intersect.iter().map(|s| s.to_string()).collect(),
        };
    vec![
        entry("Mockery", "mock", 1, &["Mockery\\MockInterface"]),
        entry("Mockery", "spy", 1, &["Mockery\\MockInterface"]),
        entry("Mockery", "namedMock", 2, &["Mockery\\MockInterface"]),
        entry("Mockery", "instanceMock", 1, &["Mockery\\MockInterface"]),
    ]
}
