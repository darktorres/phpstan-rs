//! Where the environment hands a program its values: the builtins that
//! read files, streams, the command line, the process's environment and
//! settings, and databases, and the request objects of the common
//! frameworks. A value one of them gives is anything its type allows.
//! The configuration names more (`EngineSettings::sources`).

use crate::checker::Checker;

/// Builtin functions whose result the environment chooses, lowercase and
/// sorted: what a file, a stream, a socket, the terminal, the process's
/// environment or settings, a database or a request holds, and what a
/// file's size and times are.
const SOURCE_FUNCTIONS: &[&str] = &[
    "apache_request_headers",
    "disk_free_space",
    "disk_total_space",
    "fgetc",
    "fgetcsv",
    "fgets",
    "file",
    "file_get_contents",
    "fileatime",
    "filectime",
    "fileinode",
    "filemtime",
    "filesize",
    "filter_input",
    "filter_input_array",
    "fread",
    "fscanf",
    "fstat",
    "ftell",
    "get_cfg_var",
    "getallheaders",
    "getenv",
    "getopt",
    "gzfile",
    "gzgets",
    "gzread",
    "ini_get",
    "ini_get_all",
    "lstat",
    "mysqli_fetch_all",
    "mysqli_fetch_array",
    "mysqli_fetch_assoc",
    "mysqli_fetch_column",
    "mysqli_fetch_row",
    "pg_fetch_all",
    "pg_fetch_all_columns",
    "pg_fetch_array",
    "pg_fetch_assoc",
    "pg_fetch_result",
    "pg_fetch_row",
    "readline",
    "socket_read",
    "socket_recv",
    "stat",
    "stream_get_contents",
    "stream_get_line",
    "stream_socket_recvfrom",
    "unserialize",
];

/// Methods whose result the environment chooses, by the class or
/// interface that declares them: a database result's rows, a file's
/// lines, and a request's input, headers and body in PSR-7, Symfony and
/// Laravel. Names are lowercase.
const SOURCE_METHODS: &[(&str, &[&str])] = &[
    (
        "illuminate\\http\\request",
        &[
            "all",
            "boolean",
            "collect",
            "cookie",
            "date",
            "enum",
            "except",
            "file",
            "float",
            "get",
            "getcontent",
            "header",
            "input",
            "integer",
            "ip",
            "json",
            "keys",
            "only",
            "post",
            "query",
            "route",
            "server",
            "str",
            "string",
            "validated",
        ],
    ),
    (
        "mysqli_result",
        &[
            "fetch_all",
            "fetch_array",
            "fetch_assoc",
            "fetch_column",
            "fetch_row",
        ],
    ),
    ("pdostatement", &["fetch", "fetchall", "fetchcolumn"]),
    (
        "psr\\http\\message\\messageinterface",
        &["getheader", "getheaderline", "getheaders"],
    ),
    (
        "psr\\http\\message\\serverrequestinterface",
        &[
            "getattribute",
            "getattributes",
            "getcookieparams",
            "getparsedbody",
            "getqueryparams",
            "getserverparams",
            "getuploadedfiles",
        ],
    ),
    (
        "psr\\http\\message\\streaminterface",
        &["__tostring", "getcontents", "read"],
    ),
    (
        "psr\\http\\message\\uriinterface",
        &[
            "getauthority",
            "getfragment",
            "gethost",
            "getpath",
            "getport",
            "getquery",
            "getuserinfo",
        ],
    ),
    (
        "splfileinfo",
        &["getatime", "getctime", "getinode", "getmtime", "getsize"],
    ),
    (
        "splfileobject",
        &["current", "fgetc", "fgetcsv", "fgets", "fread", "fscanf"],
    ),
    ("sqlite3result", &["fetcharray"]),
    (
        "symfony\\component\\httpfoundation\\headerbag",
        &["all", "get", "keys"],
    ),
    (
        "symfony\\component\\httpfoundation\\parameterbag",
        &[
            "all",
            "filter",
            "get",
            "getalnum",
            "getalpha",
            "getboolean",
            "getdigits",
            "getenum",
            "getint",
            "getstring",
            "keys",
        ],
    ),
    (
        "symfony\\component\\httpfoundation\\request",
        &[
            "get",
            "getclientip",
            "getcontent",
            "gethost",
            "getpassword",
            "getpathinfo",
            "getpayload",
            "getport",
            "getquerystring",
            "getrequesturi",
            "geturi",
            "getuser",
            "toarray",
        ],
    ),
];

/// The sources the configuration adds (`EngineSettings::sources`):
/// functions by name and methods as `Class::method`, both lowercase.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfiguredSources {
    pub functions: Vec<String>,
    pub methods: Vec<(String, String)>,
}

impl ConfiguredSources {
    /// From the configuration's spelling: `name` and `Class::method`.
    pub fn new(functions: &[String], methods: &[String]) -> ConfiguredSources {
        ConfiguredSources {
            functions: functions
                .iter()
                .map(|f| f.trim_start_matches('\\').to_ascii_lowercase())
                .collect(),
            methods: methods
                .iter()
                .filter_map(|m| m.split_once("::"))
                .map(|(class, method)| {
                    (
                        class.trim_start_matches('\\').to_ascii_lowercase(),
                        method.to_ascii_lowercase(),
                    )
                })
                .collect(),
        }
    }
}

impl Checker<'_> {
    /// Whether the function `name` (resolved) gives what the environment
    /// chooses; `builtin` when a stub declares it.
    pub(crate) fn is_source_function(&self, name: &str, builtin: bool) -> bool {
        let name = name.trim_start_matches('\\').to_ascii_lowercase();
        (builtin && SOURCE_FUNCTIONS.binary_search(&name.as_str()).is_ok())
            || self.settings.sources.functions.contains(&name)
    }

    /// The class `method` of `class` is a source by, as the code spells
    /// it: `class` or an ancestor of it the catalogue or the configuration
    /// names with that method.
    pub(crate) fn source_method_class(&self, class: &str, method: &str) -> Option<String> {
        let method = method.to_ascii_lowercase();
        let named = |declaring: &str| self.registry.is_subclass_of(class, declaring);
        let declaring = SOURCE_METHODS
            .iter()
            .find(|(declaring, methods)| methods.contains(&method.as_str()) && named(declaring))
            .map(|(declaring, _)| declaring.to_string())
            .or_else(|| {
                self.settings
                    .sources
                    .methods
                    .iter()
                    .find(|(declaring, m)| *m == method && named(declaring))
                    .map(|(declaring, _)| declaring.clone())
            })?;
        Some(
            self.registry
                .lookup_class(&declaring)
                .map_or(declaring, |(name, _)| name.to_string()),
        )
    }
}
