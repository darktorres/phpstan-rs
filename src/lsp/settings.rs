//! The `phpstan-rs.*` VSCode settings section, as it can reach the
//! server two different ways: pushed directly in a
//! `workspace/didChangeConfiguration` notification's own `settings`
//! payload (a legacy/non-`vscode-languageclient` client), or pulled back
//! via a `workspace/configuration` request/response round trip (what a
//! real `vscode-languageclient` client actually does, since it sends
//! `didChangeConfiguration` with an empty/placeholder `settings` value
//! and expects the server to ask for what it needs).

use serde::Deserialize;

fn default_enable() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PhpstanRsSettings {
    #[serde(default = "default_enable")]
    pub(crate) enable: bool,
    #[serde(default)]
    pub(crate) config_path: Option<String>,
}

/// Looks up the `"phpstan-rs"` section directly inside a
/// `didChangeConfiguration` notification's own `settings` value. `None`
/// when the section is absent or explicitly `null` - the common case for
/// a real `vscode-languageclient` client, which always sends `{}` here
/// and expects a `workspace/configuration` pull instead.
pub(crate) fn extract_pushed_settings(settings: &serde_json::Value) -> Option<PhpstanRsSettings> {
    let section = settings.get("phpstan-rs")?;
    if section.is_null() {
        return None;
    }
    serde_json::from_value(section.clone()).ok()
}

/// Parses a `workspace/configuration` response's `result` array - one
/// entry per requested `ConfigurationItem`, in the same order they were
/// asked for. This server only ever asks for one item (`section:
/// "phpstan-rs"`), so `result[0]` is directly that section's value, not
/// wrapped in another `"phpstan-rs"` key the way the pushed-settings tree
/// is.
pub(crate) fn parse_pull_response(result: &[serde_json::Value]) -> Option<PhpstanRsSettings> {
    let section = result.first()?;
    if section.is_null() {
        return None;
    }
    serde_json::from_value(section.clone()).ok()
}
