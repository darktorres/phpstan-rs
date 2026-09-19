//! Live SQL query reflection for raw `Doctrine\DBAL\Connection` result
//! types - result-set type inference for `fetch*`/`executeQuery`/
//! `executeStatement`, the same real-world shape `staabm/phpstan-dba`
//! uses (prepare, don't execute, and read the prepared statement's own
//! result-set column metadata). See
//! `delegated-tumbling-dongarra.md`'s sub-phases B/C/D/E for the full
//! design. Only per-call-site, `Checker`/AST-consulting logic lives
//! under this module (this file's own `query_builder`) - both A2's
//! static schema catalog and C's live reflector are compiled/lazily-
//! built state stored directly on `Registry`, so they live top-level
//! instead, in `crate::sql_reflection_schema`/`crate::
//! sql_reflection_reflector` (see either module's own doc comment for
//! why: `Registry` has zero existing references to `checker::` types,
//! and this project's `restricted_symbols`/`doctrine_functions` already
//! established that shape for exactly this kind of Registry-held data).

pub(crate) mod query_builder;
