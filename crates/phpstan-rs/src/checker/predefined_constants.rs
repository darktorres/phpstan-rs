//! Real PHPStan's `ConstantResolver::resolvePredefinedConstant`: engine
//! constants whose value depends on the PHP build/platform/runtime get a
//! general type instead of the literal phpstorm-stubs happened to record
//! when it was generated (`define('PHP_SAPI', 'cli')`,
//! `define('PHP_OS_FAMILY', 'Linux')`, `define('PHP_VERSION_ID', 50306)`).
//! Trusting those literals makes platform checks like `PHP_SAPI !==
//! 'cli'` look constant.

use crate::types::php_version::PhpVersion;
use crate::types::{Type, parse_type_string};

/// `ConstantResolver::PHP_MIN_ANALYZABLE_VERSION_ID`.
const PHP_MIN_ANALYZABLE_VERSION_ID: i64 = 50207;

/// The general type of predefined constant `name` (fully qualified, no
/// leading `\`), or `None` when its declared value is authoritative.
/// `php_version` is the configured target version, narrowing the
/// version-number constants to it.
pub(crate) fn predefined_constant_type(
    name: &str,
    php_version: Option<PhpVersion>,
) -> Option<Type> {
    let non_falsy_string = || parse_type_string("non-falsy-string");
    let ty = match name {
        "PHP_VERSION"
        | "PHP_BUILD_DATE"
        | "PHP_OS"
        | "PHP_EXTENSION_DIR"
        | "PHP_PREFIX"
        | "PHP_BINDIR"
        | "PHP_BINARY"
        | "PHP_MANDIR"
        | "PHP_LIBDIR"
        | "PHP_DATADIR"
        | "PHP_SYSCONFDIR"
        | "PHP_LOCALSTATEDIR"
        | "PHP_CONFIG_FILE_PATH"
        | "ICONV_IMPL"
        | "LIBXML_DOTTED_VERSION"
        | "PCRE_VERSION" => non_falsy_string(),
        "PHP_VERSION_ID" => match php_version {
            Some(version) => {
                let base = i64::from(version.id());
                Type::IntRange(Some(base), Some(base + 99))
            }
            None => Type::IntRange(Some(PHP_MIN_ANALYZABLE_VERSION_ID), None),
        },
        "PHP_MAJOR_VERSION" => match php_version {
            Some(version) => Type::IntLiteral(i64::from(version.major)),
            None => Type::IntRange(Some(5), None),
        },
        "PHP_MINOR_VERSION" => match php_version {
            Some(version) => Type::IntLiteral(i64::from(version.minor)),
            None => Type::IntRange(Some(0), None),
        },
        // A configured version has no patch component.
        "PHP_RELEASE_VERSION" => match php_version {
            Some(_) => Type::IntRange(Some(0), Some(99)),
            None => Type::IntRange(Some(0), None),
        },
        "PHP_ZTS" | "PHP_DEBUG" => parse_type_string("0|1"),
        "PHP_MAXPATHLEN"
        | "PHP_FLOAT_DIG"
        | "PHP_FD_SETSIZE"
        | "__COMPILER_HALT_OFFSET__"
        | "PHP_WINDOWS_VERSION_BUILD"
        | "LIBXML_VERSION"
        | "OPENSSL_VERSION_NUMBER" => Type::IntRange(Some(1), None),
        "PHP_WINDOWS_VERSION_MAJOR" => Type::IntRange(Some(4), None),
        "PHP_WINDOWS_VERSION_MINOR" => Type::IntRange(Some(0), None),
        "PHP_OS_FAMILY" => {
            parse_type_string("'Windows'|'BSD'|'Darwin'|'Solaris'|'Linux'|'Unknown'")
        }
        "PHP_SAPI" => parse_type_string(
            "'apache'|'apache2handler'|'cgi'|'cli'|'cli-server'|'embed'|'fpm-fcgi'|'litespeed'|'phpdbg'|non-falsy-string",
        ),
        "PHP_EOL" => string_literals(&["\n", "\r\n"]),
        "PHP_INT_MAX" => parse_type_string("2147483647|9223372036854775807"),
        "PHP_INT_MIN" => Type::Union(vec![
            Type::IntLiteral(i64::MIN),
            Type::IntLiteral(i64::from(i32::MIN)),
        ]),
        "PHP_INT_SIZE" => parse_type_string("4|8"),
        "PHP_SHLIB_SUFFIX" => parse_type_string("'so'|'dll'"),
        "DIRECTORY_SEPARATOR" => string_literals(&["/", "\\"]),
        "PATH_SEPARATOR" => parse_type_string("':'|';'"),
        "STDIN" | "STDOUT" | "STDERR" => parse_type_string("resource"),
        "NAN" => Type::FloatLiteral(f64::NAN),
        "INF" => Type::FloatLiteral(f64::INFINITY),
        _ => return None,
    };
    Some(ty)
}

fn string_literals(values: &[&str]) -> Type {
    Type::Union(
        values
            .iter()
            .map(|v| Type::StringLiteral((*v).to_string()))
            .collect(),
    )
}
