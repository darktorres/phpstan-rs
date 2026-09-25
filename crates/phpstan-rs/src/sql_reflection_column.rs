//! The column model SQL reflection answers in: what a raw fetch of one
//! result-set column hands back to PHP.

/// The PHP value a raw `Doctrine\DBAL\Connection::fetch*` returns for one
/// column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnType {
    Int,
    Float,
    Bool,
    String,
    /// An `int` or a `float`, varying per row (SQLite's NUMERIC affinity).
    Number,
    /// One of an `ENUM` column's declared labels.
    Enum(Vec<String>),
    /// Always `null`.
    Null,
    /// Not decidable from the column's declaration.
    Unknown,
}

/// One result-set or table column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub nullable: bool,
}
