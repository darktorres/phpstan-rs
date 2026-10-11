//! The bits of a file's `FileFacts::mentions`, and the spellings that set
//! them. The lexer notes a file's spellings as it reads them, and each walk
//! that needs one reads its bit, so the bits and their spellings live in one
//! place: a bit allocated twice, or a spelling that sets the wrong bit, would
//! not be caught where the walks read them.

/// A file that names a `phpversionaware`, `languageleveltypeaware` or
/// `phpstormstubselementavailable` attribute: the parser still resolves
/// aliases at declaration construction time, so this is a fast file fact
/// and does not decide whether the rules run.
pub(crate) const STUB_ATTRIBUTE: u32 = 1 << 0;

/// A file that names `bind`, `bindTo` or `call`: a closure rebind needs one.
pub(crate) const CLOSURE_REBIND: u32 = 1 << 1;

/// A file that names a function that tests a path, or sets the include
/// path: only such a file has an include guard to find.
pub(crate) const INCLUDE_GUARD: u32 = 1 << 2;

/// A file that names `define`, in any case, however it imports it: the only
/// call that declares a constant.
pub(crate) const DEFINE: u32 = 1 << 3;

/// A file that names `stream_wrapper_register`.
pub(crate) const STREAM_WRAPPER: u32 = 1 << 4;

/// A file that names `__FUNCTION__` or `__METHOD__`: only such a file has
/// one to find in a closure.
pub(crate) const MAGIC_CONSTANT: u32 = 1 << 5;

/// A file that names `PHPStan`: a name that resolves into the `PHPStan\`
/// namespace starts with a segment the file spells, so a file without it
/// holds none.
pub(crate) const PHPSTAN_API: u32 = 1 << 6;

/// A file that names something that writes a property without a write in
/// the code: reflection, the fetch modes that fill an object,
/// unserialisation, or a wrapper that exposes an object's properties as
/// array elements. Names are also noted when a string is one, as a callable
/// is.
pub(crate) const PROPERTY_WRITER: u32 = 1 << 7;

const _: () = {
    let bits = [
        STUB_ATTRIBUTE,
        CLOSURE_REBIND,
        INCLUDE_GUARD,
        DEFINE,
        STREAM_WRAPPER,
        MAGIC_CONSTANT,
        PHPSTAN_API,
        PROPERTY_WRITER,
    ];
    let mut i = 0;
    while i < bits.len() {
        assert!(bits[i].is_power_of_two());
        i += 1;
    }
    let all = STUB_ATTRIBUTE
        | CLOSURE_REBIND
        | INCLUDE_GUARD
        | DEFINE
        | STREAM_WRAPPER
        | MAGIC_CONSTANT
        | PHPSTAN_API
        | PROPERTY_WRITER;
    assert!(
        all.count_ones() == bits.len() as u32,
        "two bits are the same"
    );
};

const STUB_ATTRIBUTE_SPELLINGS: &[(&str, u32)] = &[
    ("phpversionaware", STUB_ATTRIBUTE),
    ("languageleveltypeaware", STUB_ATTRIBUTE),
    ("phpstormstubselementavailable", STUB_ATTRIBUTE),
];

const CLOSURE_REBIND_SPELLINGS: &[(&str, u32)] = &[
    ("bind", CLOSURE_REBIND),
    ("bindto", CLOSURE_REBIND),
    ("call", CLOSURE_REBIND),
];

const INCLUDE_GUARD_SPELLINGS: &[(&str, u32)] = &[
    ("file_exists", INCLUDE_GUARD),
    ("is_file", INCLUDE_GUARD),
    ("is_readable", INCLUDE_GUARD),
    ("is_writable", INCLUDE_GUARD),
    ("is_writeable", INCLUDE_GUARD),
    ("is_executable", INCLUDE_GUARD),
    ("set_include_path", INCLUDE_GUARD),
    ("ini_set", INCLUDE_GUARD),
];

const DEFINE_SPELLINGS: &[(&str, u32)] = &[("define", DEFINE)];

const STREAM_WRAPPER_SPELLINGS: &[(&str, u32)] = &[("stream_wrapper_register", STREAM_WRAPPER)];

const MAGIC_CONSTANT_SPELLINGS: &[(&str, u32)] = &[
    ("__function__", MAGIC_CONSTANT),
    ("__method__", MAGIC_CONSTANT),
];

const PHPSTAN_API_SPELLINGS: &[(&str, u32)] = &[("phpstan", PHPSTAN_API)];

pub(crate) const PROPERTY_WRITER_SPELLINGS: &[(&str, u32)] = &[
    ("setvalue", PROPERTY_WRITER),
    ("setrawvaluewithoutlazyinitialization", PROPERTY_WRITER),
    ("unserialize", PROPERTY_WRITER),
    ("igbinary_unserialize", PROPERTY_WRITER),
    ("msgpack_unserialize", PROPERTY_WRITER),
    ("_unserialize", PROPERTY_WRITER),
    ("fetchobject", PROPERTY_WRITER),
    ("fetch_object", PROPERTY_WRITER),
    ("mysqli_fetch_object", PROPERTY_WRITER),
    ("cubrid_fetch_object", PROPERTY_WRITER),
    ("db2_fetch_object", PROPERTY_WRITER),
    ("fbird_fetch_object", PROPERTY_WRITER),
    ("ibase_fetch_object", PROPERTY_WRITER),
    ("mssql_fetch_object", PROPERTY_WRITER),
    ("mysql_fetch_object", PROPERTY_WRITER),
    ("oci_fetch_object", PROPERTY_WRITER),
    ("odbc_fetch_object", PROPERTY_WRITER),
    ("pg_fetch_object", PROPERTY_WRITER),
    ("sqlite_fetch_object", PROPERTY_WRITER),
    ("sqlsrv_fetch_object", PROPERTY_WRITER),
    ("swoole_substr_unserialize", PROPERTY_WRITER),
    ("sybase_fetch_object", PROPERTY_WRITER),
    ("fetch_class", PROPERTY_WRITER),
    ("fetch_into", PROPERTY_WRITER),
    ("fetch_props_late", PROPERTY_WRITER),
    ("setfetchmode", PROPERTY_WRITER),
    ("arrayobject", PROPERTY_WRITER),
    ("arrayiterator", PROPERTY_WRITER),
    ("recursivearrayiterator", PROPERTY_WRITER),
];

/// Every spelling the parser watches for, with the bit it sets.
pub(crate) static WATCHED: syntax::WatchList = syntax::WatchList(&[
    STUB_ATTRIBUTE_SPELLINGS,
    CLOSURE_REBIND_SPELLINGS,
    INCLUDE_GUARD_SPELLINGS,
    DEFINE_SPELLINGS,
    STREAM_WRAPPER_SPELLINGS,
    MAGIC_CONSTANT_SPELLINGS,
    PHPSTAN_API_SPELLINGS,
    PROPERTY_WRITER_SPELLINGS,
]);
