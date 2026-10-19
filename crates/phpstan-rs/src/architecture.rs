//! Declarative architecture rules (`[[architecture]]` in the config
//! file): which classes may depend on, extend, implement, construct, or
//! look like what - checked by `Project::check_architecture`.
//!
//! ```toml
//! [[architecture]]
//! name = "the domain stays framework-free"
//! classes = [{ namespace = "App\\Domain" }]
//! excluding = [{ classname = "App\\Domain\\Kernel" }]
//! should_not_depend_on = [{ namespace = "App\\Infrastructure" }, { namespace = "Symfony" }]
//!
//! [[architecture]]
//! classes = [{ namespace = "App\\Controller" }]
//! should_be_final = true
//! should_be_named = "/Controller$/"
//! ```
//!
//! A selector's fields must all hold (and none of its own `excluding`
//! selectors match); a list of selectors matches when any one does. A
//! `namespace`/`classname`/`should_be_named` value written as `/.../` is
//! a regular expression, otherwise a namespace prefix / exact name.

use serde::Deserialize;

/// `[[rules.architecture]]`: every rule, in order.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct ArchitectureSettings(pub Vec<ArchitectureRule>);

impl config::Section for ArchitectureSettings {
    const PATH: &'static [&'static str] = &["rules", "architecture"];
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub namespace: Option<String>,
    pub classname: Option<String>,
    /// The class extends this one, directly or not.
    pub extends: Option<String>,
    /// The class implements this interface, directly or not.
    pub implements: Option<String>,
    pub uses_trait: Option<String>,
    pub applies_attribute: Option<String>,
    pub is_abstract: Option<bool>,
    pub is_final: Option<bool>,
    pub is_readonly: Option<bool>,
    pub is_interface: Option<bool>,
    pub is_trait: Option<bool>,
    pub is_enum: Option<bool>,
    pub is_exception: Option<bool>,
    pub is_attribute: Option<bool>,
    /// A concrete (non-abstract) class, not an interface/trait/enum.
    pub is_standard_class: Option<bool>,
    #[serde(default)]
    pub excluding: Vec<Selector>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureRule {
    /// Shown in diagnostics; defaults to a description of the relation.
    pub name: Option<String>,
    /// The classes this rule constrains.
    pub classes: Vec<Selector>,
    #[serde(default)]
    pub excluding: Vec<Selector>,
    #[serde(default)]
    pub should_not_depend_on: Vec<Selector>,
    /// Every dependency must match one of these (built-in classes and
    /// the class itself always allowed).
    pub can_only_depend_on: Option<Vec<Selector>>,
    #[serde(default)]
    pub should_depend_on: Vec<Selector>,
    #[serde(default)]
    pub should_not_construct: Vec<Selector>,
    #[serde(default)]
    pub should_extend: Vec<Selector>,
    #[serde(default)]
    pub should_not_extend: Vec<Selector>,
    #[serde(default)]
    pub should_implement: Vec<Selector>,
    #[serde(default)]
    pub should_not_implement: Vec<Selector>,
    #[serde(default)]
    pub should_use_trait: Vec<Selector>,
    #[serde(default)]
    pub should_not_use_trait: Vec<Selector>,
    pub should_be_final: Option<bool>,
    pub should_be_abstract: Option<bool>,
    pub should_be_readonly: Option<bool>,
    pub should_be_named: Option<String>,
    pub should_apply_attribute: Option<String>,
    pub should_have_only_one_public_method: Option<bool>,
}

/// A `namespace`/`classname`/`should_be_named` pattern.
pub(crate) enum NamePattern {
    Regex(regex::Regex),
    Text(String),
}

impl NamePattern {
    pub(crate) fn parse(text: &str) -> Result<NamePattern, String> {
        if text.len() >= 2 && text.starts_with('/') && text.ends_with('/') {
            regex::Regex::new(&text[1..text.len() - 1])
                .map(NamePattern::Regex)
                .map_err(|e| format!("invalid architecture pattern {text}: {e}"))
        } else {
            Ok(NamePattern::Text(text.trim_start_matches('\\').to_string()))
        }
    }

    /// A namespace pattern: the class is in the namespace or below it.
    pub(crate) fn matches_namespace(&self, fqn: &str) -> bool {
        match self {
            NamePattern::Regex(re) => re.is_match(fqn),
            NamePattern::Text(prefix) => {
                let prefix = prefix.trim_end_matches('\\');
                fqn.len() > prefix.len()
                    && fqn[..prefix.len()].eq_ignore_ascii_case(prefix)
                    && fqn.as_bytes()[prefix.len()] == b'\\'
            }
        }
    }

    /// A class-name pattern: the exact (case-insensitive) name.
    pub(crate) fn matches_name(&self, fqn: &str) -> bool {
        match self {
            NamePattern::Regex(re) => re.is_match(fqn),
            NamePattern::Text(name) => name.eq_ignore_ascii_case(fqn),
        }
    }
}

impl ArchitectureRule {
    /// Every pattern in the rule, parsed, so a bad one is reported once
    /// at startup.
    pub fn validate(&self) -> Result<(), String> {
        fn selectors_ok(selectors: &[Selector]) -> Result<(), String> {
            stack::guard(|| {
                for s in selectors {
                    for pattern in [&s.namespace, &s.classname].into_iter().flatten() {
                        NamePattern::parse(pattern)?;
                    }
                    selectors_ok(&s.excluding)?;
                }
                Ok(())
            })
        }
        if self.classes.is_empty() {
            return Err("an [[architecture]] rule needs a non-empty `classes`".to_string());
        }
        for list in [
            &self.classes,
            &self.excluding,
            &self.should_not_depend_on,
            &self.should_depend_on,
            &self.should_not_construct,
            &self.should_extend,
            &self.should_not_extend,
            &self.should_implement,
            &self.should_not_implement,
            &self.should_use_trait,
            &self.should_not_use_trait,
        ] {
            selectors_ok(list)?;
        }
        if let Some(list) = &self.can_only_depend_on {
            selectors_ok(list)?;
        }
        if let Some(name) = &self.should_be_named {
            NamePattern::parse(name)?;
        }
        Ok(())
    }
}
