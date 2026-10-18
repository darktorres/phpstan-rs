//! The reflection's keys: which PHP versions the project runs on, what's
//! loaded beside the analysed files for context, and where its includes
//! are found.

use std::path::PathBuf;

use serde::Deserialize;

/// Top-level keys.
#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ReflectionSettings {
    /// The PHP version, as `"major.minor"`.
    pub php_version: Option<String>,
    /// Directories loaded for type resolution only, never analysed.
    pub scan_paths: Vec<PathBuf>,
    /// Directories or files whose class-like declarations are designated
    /// overrides (a vendored `Imagick.stub.php`) rather than accidental
    /// duplicates; loaded for context like `scan_paths`.
    pub stub_paths: Vec<PathBuf>,
    /// Turns off loading `vendor/` through Composer's autoloader.
    pub no_composer: Option<bool>,
    /// The deployment's `include_path`, in its syntax (`.:/usr/share/php`,
    /// `.;C:\php\pear`). The values that `set_include_path()` and
    /// `ini_set('include_path', ...)` give it in the analysed files, and the
    /// `include-path` directories of Composer's packages, are searched too.
    /// Undeclared, the start of the path is anything, and an `include` of a
    /// bare name (`require 'x.php'`) is not judged. An entry outside the
    /// project does the same: declare `.` or a directory of the project to
    /// have bare names judged.
    pub include_path: Option<String>,
    /// The directories a run can start in, relative to the config file.
    /// Undeclared, a run starts in the root, as a command-line run does, or
    /// in the directory of any analysed file, as a web server does for the
    /// script it runs. An `include` of `./x.php` is looked up there. A
    /// `chdir()` in the analysed files adds the directory it names; one whose
    /// directory is not known leaves every file relative to the working
    /// directory unjudged.
    pub working_dirs: Option<Vec<PathBuf>>,
}

impl config::Section for ReflectionSettings {
    const PATH: &'static [&'static str] = &[];
}
