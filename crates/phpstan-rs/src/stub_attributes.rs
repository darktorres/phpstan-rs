//! Lexer watch spellings for the parser-time phpstorm-stubs rules.

/// The spellings that let file products cheaply note likely stub attributes.
/// The parser still resolves aliases at declaration construction time, so
/// the watch is only a fast file fact and does not decide whether rules run.
pub(crate) const MENTIONS: u32 = 1 << 0;

pub(crate) const WATCH: &[(&str, u32)] = &[
    ("phpversionaware", MENTIONS),
    ("languageleveltypeaware", MENTIONS),
    ("phpstormstubselementavailable", MENTIONS),
];
