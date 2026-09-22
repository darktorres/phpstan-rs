//! Sub-phase A2: a static schema catalog parsed from a per-engine DDL
//! dump (`install/piwigo_structure-{mysql,mariadb,pgsql,sqlite}.sql`-
//! shaped), the same per-column type-translation table the live
//! reflector (sub-phase C) will use - just fed from a parsed `sqlparser`
//! AST `ColumnDef` instead of a live prepared-statement response. Not
//! merely a fallback for when a live DB is unavailable - versioned in
//! the repo, no live infrastructure needed, and immune to a locally-
//! running DB's own schema having drifted from what's actually
//! committed.
//!
//! **Real parsing risk, confirmed directly against all 4 real DDL
//! dumps, not assumed clean**: naively feeding a whole dump straight to
//! `sqlparser::parser::Parser::parse_sql` fails on 3 of the 4 engines -
//! not because comments aren't stripped (they are), but because two
//! deliberate `sqlparser` behaviors surface real grammar gaps this
//! project doesn't actually need to work around:
//! - MySQL/MariaDB: `sqlparser`'s tokenizer deliberately *un*wraps
//!   MySQL's own `/*!VERSION ... */` "executable comment" syntax into
//!   real tokens (confirmed reading `Tokenizer::
//!   tokenize_comment_hints`) rather than treating it as inert -
//!   faithful to how a real MySQL server behaves, but it surfaces a
//!   real gap in `sqlparser` 0.63.0's own `FullTextOrSpatialConstraint`
//!   grammar (no `WITH PARSER <name>` support at all), which the real
//!   dump's `FULLTEXT KEY ... /*!50100 WITH PARSER \`ngram\` */` hits.
//!   This project never needs a full-text parser plugin name for column
//!   type/nullability extraction, so `strip_mysql_version_hint_comments`
//!   removes these spans before parsing rather than waiting on
//!   `sqlparser` to support every clause MySQL allows inside them.
//! - Postgres/SQLite: a handful of individual statements this project
//!   doesn't need at all fail outright under this `sqlparser` version -
//!   confirmed real, not hypothetical: Postgres's own inline-sequence
//!   `ALTER TABLE ... ADD GENERATED ... AS IDENTITY (SEQUENCE NAME
//!   ...)` syntax, and SQLite's own `CREATE VIRTUAL TABLE ... USING
//!   fts5(...)` syntax. Splitting the file into independent per-
//!   statement token chunks (at the tokenizer level, not naive text
//!   splitting, so quoted/commented semicolons are never mistaken for a
//!   statement boundary) and parsing each chunk on its own means one
//!   unparseable statement never aborts the whole file - a chunk this
//!   catalog doesn't need (an `ALTER TABLE`, `CREATE VIEW`, `CREATE
//!   TRIGGER`, `COMMENT ON`, ...) failing to parse is simply skipped,
//!   not a hard error. Verified end to end: this combination extracts
//!   exactly 38 `CREATE TABLE` statements from every one of the 4 real
//!   dumps.

use std::path::Path;

use rustc_hash::FxHashMap;
use sqlparser::ast::{
    ColumnDef, ColumnOption, ColumnOptionDef, DataType, EnumMember, Expr, ObjectName,
    ObjectNamePart, Statement, TableConstraint,
};
use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use crate::sql_reflection_config::SqlDriver;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub struct SchemaColumn {
    pub name: String,
    pub ty: Type,
    pub nullable: bool,
}

/// `table name -> its own columns, in declared order`. Table/column
/// names are kept exactly as declared (MySQL/Postgres/SQLite table
/// names are all case-sensitive by default on this project's own target
/// platforms) - no case-folding.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SchemaCatalog {
    tables: FxHashMap<String, Vec<SchemaColumn>>,
}

impl SchemaCatalog {
    pub fn table(&self, name: &str) -> Option<&[SchemaColumn]> {
        self.tables.get(name).map(Vec::as_slice)
    }

    pub fn column(&self, table: &str, column: &str) -> Option<&SchemaColumn> {
        self.table(table)?.iter().find(|c| c.name == column)
    }

    /// Every declared table's own name - the schema drift check's own
    /// iteration surface (`sql_reflection_schema_drift`).
    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.tables.keys().map(String::as_str)
    }
}

pub fn parse_ddl_file(path: &Path, driver: SqlDriver) -> Result<SchemaCatalog, String> {
    let sql = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    Ok(parse_ddl(&sql, driver))
}

pub fn parse_ddl(sql: &str, driver: SqlDriver) -> SchemaCatalog {
    let dialect: Box<dyn Dialect> = match driver {
        SqlDriver::Mysql => Box::new(MySqlDialect {}),
        SqlDriver::Pgsql => Box::new(PostgreSqlDialect {}),
        SqlDriver::Sqlite => Box::new(SQLiteDialect {}),
    };
    // Harmless (a no-op) for a dump that never contains `/*!` at all -
    // applied unconditionally rather than gated on `driver` so a
    // MariaDB dump fed in under `SqlDriver::Mysql` (the same dialect
    // value the config surface uses for both) is covered too.
    let sql = strip_mysql_version_hint_comments(sql);

    let mut tables = FxHashMap::default();
    for create_table in create_table_statements(&sql, dialect.as_ref()) {
        let table_name = object_name_to_string(&create_table.name);
        if table_name.is_empty() {
            continue;
        }
        let primary_key_columns = primary_key_column_names(&create_table.constraints);
        let columns = create_table
            .columns
            .iter()
            .map(|col| schema_column(col, &primary_key_columns))
            .collect();
        tables.insert(table_name, columns);
    }
    SchemaCatalog { tables }
}

fn strip_mysql_version_hint_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut rest = sql;
    while let Some(start) = rest.find("/*!") {
        out.push_str(&rest[..start]);
        match rest[start..].find("*/") {
            Some(end) => rest = &rest[start + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn create_table_statements(sql: &str, dialect: &dyn Dialect) -> Vec<sqlparser::ast::CreateTable> {
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize() else {
        return Vec::new();
    };
    let mut chunks: Vec<Vec<Token>> = Vec::new();
    let mut chunk = Vec::new();
    for token in tokens {
        if token == Token::SemiColon {
            if !chunk.is_empty() {
                chunks.push(std::mem::take(&mut chunk));
            }
        } else {
            chunk.push(token);
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    let mut out = Vec::new();
    for chunk in chunks {
        let Ok(stmts) = Parser::new(dialect).with_tokens(chunk).parse_statements() else {
            continue;
        };
        for stmt in stmts {
            if let Statement::CreateTable(create_table) = stmt {
                out.push(create_table);
            }
        }
    }
    out
}

fn object_name_to_string(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident.value.clone()),
            ObjectNamePart::Function(_) => None,
        })
        .unwrap_or_default()
}

/// Columns named by a table-level `PRIMARY KEY (...)` constraint are
/// implicitly `NOT NULL` by real SQL rule, even on a real dump that
/// doesn't also spell out `NOT NULL` on the column itself.
fn primary_key_column_names(constraints: &[TableConstraint]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    for constraint in constraints {
        if let TableConstraint::PrimaryKey(pk) = constraint {
            for indexed in &pk.columns {
                if let Expr::Identifier(ident) = &indexed.column.expr {
                    names.insert(ident.value.clone());
                }
            }
        }
    }
    names
}

fn column_is_not_null(options: &[ColumnOptionDef]) -> bool {
    options.iter().any(|o| {
        matches!(o.option, ColumnOption::NotNull) || matches!(o.option, ColumnOption::PrimaryKey(_))
    })
}

fn schema_column(
    col: &ColumnDef,
    primary_key_columns: &std::collections::HashSet<String>,
) -> SchemaColumn {
    let nullable =
        !(column_is_not_null(&col.options) || primary_key_columns.contains(&col.name.value));
    SchemaColumn {
        name: col.name.value.clone(),
        ty: data_type_to_type(&col.data_type),
        nullable,
    }
}

/// The shared "declared SQL column type -> raw PHP fetch type" table -
/// what a real `Doctrine\DBAL\Connection::fetch*` call actually hands
/// back to PHP for a column of this declared type, not what the
/// database's own internal storage representation is. Every rule here
/// is either a direct SQL-level fact (an `ENUM`'s value is always
/// returned as its declared label) or a driver-level fetch-format fact
/// (confirmed live against real MySQL/PDO where noted) - never a guess
/// dressed up as either. A declared type this table has no confident
/// answer for degrades to `Type::Mixed(false)`, matching this project's own
/// established "honest, not invented" precedent elsewhere (see
/// `array_keys()`'s own `Type::Mixed(false)` fallback,
/// `checker/expr/builtin_calls.rs`).
fn data_type_to_type(data_type: &DataType) -> Type {
    match data_type {
        DataType::Enum(members, _) => enum_member_union(members),
        // A raw DBAL fetch never decodes JSON - confirmed against
        // MySQL's own wire protocol, which returns a JSON column as its
        // raw string content, not a value `json_decode()` already ran
        // on.
        DataType::JSON | DataType::JSONB => Type::String,
        // Date/time family: a raw fetch always returns PHP's own
        // textual representation, never a `DateTime` object - that
        // conversion is a `Doctrine\DBAL\Types::DATETIME_*`-level
        // concern this project's own raw-fetch call sites never apply
        // implicitly.
        DataType::Date
        | DataType::Date32
        | DataType::Time(..)
        | DataType::Datetime(..)
        | DataType::Datetime64(..)
        | DataType::Timestamp(..)
        | DataType::TimestampNtz(..) => Type::String,
        // mysqlnd returns DECIMAL/NUMERIC as a string unconditionally -
        // confirmed live against a real MySQL instance across every
        // `PDO::ATTR_EMULATE_PREPARES` setting, so this is decidable
        // statically, unlike FLOAT/DOUBLE just below.
        DataType::Numeric(_)
        | DataType::Decimal(_)
        | DataType::DecimalUnsigned(_)
        | DataType::BigNumeric(_)
        | DataType::BigDecimal(_)
        | DataType::Dec(_)
        | DataType::DecUnsigned(_) => Type::String,
        // FLOAT/DOUBLE (and, below, Postgres `boolean`) switch on
        // `PDO::ATTR_STRINGIFY_FETCHES` (string when true, native type
        // when false) - **correction, verified live**: PDO's own real
        // default for this attribute is `false` (confirmed directly:
        // `(new PDO(...))->getAttribute(PDO::ATTR_STRINGIFY_FETCHES)`
        // returns `false` with nothing else set), and neither Doctrine
        // DBAL nor this corpus's own application code ever sets it
        // anywhere (confirmed: zero matches for `ATTR_STRINGIFY_FETCHES`
        // across `vendor/doctrine/dbal/src` and this corpus's own `src`/
        // `config`/`tests`) - so the real, unmodified, in-practice
        // answer for this project's actual target is the native type,
        // not an honestly-undecided union. This is a real, checked
        // assumption about this specific stack, not a blind default -
        // see the plan's own "detect/match the app's own real setting"
        // framing, which a from-scratch project without this exact
        // grep-confirmed non-interference would need to earn separately.
        DataType::Float(_)
        | DataType::FloatUnsigned(_)
        | DataType::Float4
        | DataType::Float32
        | DataType::Float64
        | DataType::Real
        | DataType::RealUnsigned
        | DataType::Float8
        | DataType::Double(_)
        | DataType::DoubleUnsigned(_)
        | DataType::DoublePrecision
        | DataType::DoublePrecisionUnsigned => Type::Float,
        DataType::Bool | DataType::Boolean => Type::Bool,
        DataType::TinyInt(_)
        | DataType::TinyIntUnsigned(_)
        | DataType::UTinyInt
        | DataType::Int2(_)
        | DataType::Int2Unsigned(_)
        | DataType::SmallInt(_)
        | DataType::SmallIntUnsigned(_)
        | DataType::USmallInt
        | DataType::MediumInt(_)
        | DataType::MediumIntUnsigned(_)
        | DataType::Int(_)
        | DataType::Int4(_)
        | DataType::Int8(_)
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::Int128
        | DataType::Int256
        | DataType::Integer(_)
        | DataType::IntUnsigned(_)
        | DataType::Int4Unsigned(_)
        | DataType::IntegerUnsigned(_)
        | DataType::HugeInt
        | DataType::UHugeInt
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::UInt128
        | DataType::UInt256
        | DataType::BigInt(_)
        | DataType::BigIntUnsigned(_)
        | DataType::UBigInt
        | DataType::Int8Unsigned(_) => Type::Int,
        // `TINYINT(1)` is deliberately here, not treated as `Bool` -
        // confirmed live against real PDO in both
        // `PDO::ATTR_EMULATE_PREPARES` settings: it always comes back
        // as plain PHP `int`, never `bool`. There is no driver ground
        // truth for a `Bool` mapping here at all.
        DataType::Character(_)
        | DataType::Char(_)
        | DataType::CharacterVarying(_)
        | DataType::CharVarying(_)
        | DataType::Varchar(_)
        | DataType::Nvarchar(_)
        | DataType::Uuid
        | DataType::CharacterLargeObject(_)
        | DataType::CharLargeObject(_)
        | DataType::Clob(_)
        | DataType::Text
        | DataType::TinyText
        | DataType::MediumText
        | DataType::LongText
        | DataType::String(_)
        | DataType::FixedString(_)
        | DataType::Binary(_)
        | DataType::Varbinary(_)
        | DataType::Blob(_)
        | DataType::TinyBlob
        | DataType::MediumBlob
        | DataType::LongBlob
        | DataType::Bytes(_)
        | DataType::Bytea
        | DataType::Bit(_)
        | DataType::BitVarying(_)
        | DataType::VarBit(_) => Type::String,
        // A dialect-specific type this table has no first-class
        // translation for (native arrays, geometric types, tuples,
        // custom/domain types, ...) - genuinely undecidable from a
        // static definition alone.
        _ => Type::Mixed(false),
    }
}

/// MySQL returns an `ENUM` column's value as its declared string label,
/// so the honest answer is a literal string union of the exact declared
/// values, not bare `Type::String` - this project's own literal-string-
/// union support already makes that precision possible. `EnumMember`
/// has two real shapes (`Name(String)` and ClickHouse's own
/// `NamedValue(String, Expr)`), confirmed directly against `sqlparser`
/// 0.63.0's real AST rather than assumed to be a flat `Vec<String>`;
/// both carry a real label string, so both map the same way here.
fn enum_member_union(members: &[EnumMember]) -> Type {
    let literals: Vec<Type> = members
        .iter()
        .map(|m| match m {
            EnumMember::Name(s) | EnumMember::NamedValue(s, _) => Type::StringLiteral(s.clone()),
        })
        .collect();
    if literals.is_empty() {
        Type::String
    } else {
        Type::Union(literals)
    }
}

#[cfg(test)]
mod tests;
