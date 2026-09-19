//! Live SQL query reflection for raw `Doctrine\DBAL\Connection` result
//! types - result-set type inference for `fetch*`/`executeQuery`/
//! `executeStatement`, the same real-world shape `staabm/phpstan-dba`
//! uses (prepare, don't execute, and read the prepared statement's own
//! result-set column metadata). See
//! `delegated-tumbling-dongarra.md`'s sub-phases B/C/D/E for the full
//! design (A2's own static schema catalog lives top-level, in
//! `crate::sql_reflection_schema` - see that module's own doc comment
//! for why).

pub(crate) mod query_builder;
