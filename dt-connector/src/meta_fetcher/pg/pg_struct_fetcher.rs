use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::bail;
use dt_common::meta::struct_meta::{
    statement::{
        pg_create_rbac_statement::PgCreateRbacStatement,
        pg_create_routine_statement::{PgCreateRoutineStatement, PgRoutineKind},
        pg_create_schema_statement::PgCreateSchemaStatement,
        pg_create_table_statement::PgCreateTableStatement,
        pg_create_view_statement::{PgCreateViewStatement, PgViewKind},
    },
    structure::{
        column::{Column, ColumnDefault},
        comment::{Comment, CommentType},
        constraint::{Constraint, ConstraintType},
        index::{Index, IndexKind},
        rbac::{PgPrivilege, PgRole, PgRoleMember},
        schema::Schema,
        sequence::Sequence,
        sequence_owner::SequenceOwner,
        table::Table,
    },
};
use dt_common::{
    config::{config_enums::DbType, config_token_parser::ConfigTokenParser},
    error::Error,
    log_error, log_info, log_warn,
    rdb_filter::RdbFilter,
    utils::sql_util::SqlUtil,
};
use futures::TryStreamExt;
use sqlx::{postgres::PgRow, Pool, Postgres, Row};

use super::pg_struct_check_fetcher::PgStructCheckFetcher;

pub struct PgStructFetcher {
    pub conn_pool: Pool<Postgres>,
    pub db_type: DbType,
    pub schemas: HashSet<String>,
    pub filter: Option<RdbFilter>,
}

enum ColType {
    Text,
    Char,
}

impl PgStructFetcher {
    pub async fn get_create_schema_statements(
        &mut self,
        sch: &str,
    ) -> anyhow::Result<Vec<PgCreateSchemaStatement>> {
        let schemas = self.get_schemas(sch).await?;
        Ok(schemas
            .into_iter()
            .map(|s| PgCreateSchemaStatement { schema: s })
            .collect())
    }

    pub async fn get_create_table_statements(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<Vec<PgCreateTableStatement>> {
        let mut results = Vec::new();

        let tables = self.get_tables(sch, tb).await?;
        let mut sequences = self.get_sequences(sch, tb).await?;
        let mut sequence_owners = self.get_sequence_owners(sch, tb).await?;
        let mut constraints = self.get_constraints(sch, tb).await?;
        let mut indexes = self.get_indexes(sch, tb).await?;
        let mut column_comments = self.get_column_comments(sch, tb).await?;
        let mut table_comments = self.get_table_comments(sch, tb).await?;

        for (schema_table_name, table) in tables {
            let table_sequences = self.get_table_sequences(&table, &mut sequences).await?;
            let statement = PgCreateTableStatement {
                table,
                sequences: table_sequences,
                sequence_owners: self.get_result(&mut sequence_owners, &schema_table_name),
                constraints: self.get_result(&mut constraints, &schema_table_name),
                indexes: self.get_result(&mut indexes, &schema_table_name),
                column_comments: self.get_result(&mut column_comments, &schema_table_name),
                table_comments: self.get_result(&mut table_comments, &schema_table_name),
            };
            results.push(statement);
        }
        Ok(results)
    }

    pub async fn get_create_routine_statements(
        &mut self,
        sch: &str,
        routine: &str,
    ) -> anyhow::Result<Vec<PgCreateRoutineStatement>> {
        let mut results = Vec::new();

        let filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !routine.is_empty() {
                format!("n.nspname='{}' AND p.proname = '{}'", sch, routine)
            } else {
                format!("n.nspname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("n.nspname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        // GaussDB returns `pg_get_functiondef(oid)` as a record:
        // (headerlines int, definition text). Use `.definition` to get the text definition.
        let create_sql_expr = if matches!(self.db_type, DbType::GaussDBPg) {
            "(pg_get_functiondef(p.oid)).definition"
        } else {
            "pg_get_functiondef(p.oid)"
        };

        let base_sql = format!(
            "SELECT n.nspname AS schema_name,
                    p.proname AS routine_name,
                    l.lanname AS language,
                    {} AS create_sql
             FROM pg_proc p
             JOIN pg_namespace n ON n.oid = p.pronamespace
             JOIN pg_language l ON l.oid = p.prolang
             WHERE {}",
            create_sql_expr, filter
        );

        // Prefer `prokind` (PG >= 11) to distinguish function vs procedure. Fallback when missing.
        let sql_with_prokind = format!(
            "SELECT n.nspname AS schema_name,
                    p.proname AS routine_name,
                    l.lanname AS language,
                    p.prokind::text AS prokind,
                    {} AS create_sql
             FROM pg_proc p
             JOIN pg_namespace n ON n.oid = p.pronamespace
             JOIN pg_language l ON l.oid = p.prolang
             WHERE {}",
            create_sql_expr, filter
        );

        let with_prokind_res: anyhow::Result<()> = async {
            let mut rows = sqlx::query(&sql_with_prokind).fetch(&self.conn_pool);
            while let Some(row) = rows.try_next().await? {
                let schema_name = Self::get_str_with_null(&row, "schema_name")?;
                let routine_name = Self::get_str_with_null(&row, "routine_name")?;
                let language = Self::get_str_with_null(&row, "language")?;
                let prokind = Self::get_str_with_null(&row, "prokind")?;
                let create_sql = Self::get_str_with_null(&row, "create_sql")?;

                if self.filter_tb(&schema_name, &routine_name) {
                    continue;
                }

                let lang_lower = language.to_lowercase();
                if lang_lower != "plpgsql" && lang_lower != "sql" {
                    log_warn!(
                        "skip routine (unsupported language): {}.{}, language={}",
                        schema_name,
                        routine_name,
                        language
                    );
                    continue;
                }

                let kind = match prokind.as_str() {
                    "p" => PgRoutineKind::Procedure,
                    "f" => PgRoutineKind::Function,
                    // skip aggregates/window/other kinds
                    _ => {
                        log_warn!(
                            "skip routine (unsupported prokind): {}.{}, prokind={}",
                            schema_name,
                            routine_name,
                            prokind
                        );
                        continue;
                    }
                };

                results.push(PgCreateRoutineStatement {
                    schema_name,
                    routine_name,
                    kind,
                    create_sql,
                });
            }
            Ok(())
        }
        .await;

        match with_prokind_res {
            Ok(()) => Ok(results),
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("prokind") {
                    log_warn!(
                        "routine extraction fallback (pg_proc.prokind unavailable), error: {}",
                        e
                    );
                    results.clear();
                    let mut rows = sqlx::query(&base_sql).fetch(&self.conn_pool);
                    while let Some(row) = rows.try_next().await? {
                        let schema_name = Self::get_str_with_null(&row, "schema_name")?;
                        let routine_name = Self::get_str_with_null(&row, "routine_name")?;
                        let language = Self::get_str_with_null(&row, "language")?;
                        let create_sql = Self::get_str_with_null(&row, "create_sql")?;

                        if self.filter_tb(&schema_name, &routine_name) {
                            continue;
                        }

                        let lang_lower = language.to_lowercase();
                        if lang_lower != "plpgsql" && lang_lower != "sql" {
                            log_warn!(
                                "skip routine (unsupported language): {}.{}, language={}",
                                schema_name,
                                routine_name,
                                language
                            );
                            continue;
                        }

                        // If `prokind` is unavailable, infer routine kind from the CREATE header.
                        let header = create_sql.trim_start().to_uppercase();
                        let kind = if header.starts_with("CREATE OR REPLACE PROCEDURE")
                            || header.starts_with("CREATE PROCEDURE")
                        {
                            PgRoutineKind::Procedure
                        } else {
                            PgRoutineKind::Function
                        };

                        results.push(PgCreateRoutineStatement {
                            schema_name,
                            routine_name,
                            kind,
                            create_sql,
                        });
                    }
                    Ok(results)
                } else {
                    Err(e)
                }
            }
        }
    }

    pub async fn get_create_view_statements(
        &mut self,
        sch: &str,
        view: &str,
    ) -> anyhow::Result<Vec<PgCreateViewStatement>> {
        let mut results = Vec::new();

        let filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !view.is_empty() {
                format!("schemaname='{}' AND viewname = '{}'", sch, view)
            } else {
                format!("schemaname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("schemaname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        // ordinary views
        let sql = format!(
            "SELECT schemaname, viewname, definition
             FROM pg_views
             WHERE {}",
            filter
        );
        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let schema_name = Self::get_str_with_null(&row, "schemaname")?;
            let view_name = Self::get_str_with_null(&row, "viewname")?;
            let definition = Self::get_str_with_null(&row, "definition")?;
            if self.filter_tb(&schema_name, &view_name) {
                continue;
            }
            results.push(PgCreateViewStatement {
                schema_name,
                view_name,
                kind: PgViewKind::View,
                definition,
            });
        }

        // materialized views
        let sql = format!(
            "SELECT schemaname, matviewname AS viewname, definition
             FROM pg_matviews
             WHERE {}",
            filter
        );
        let matview_res: anyhow::Result<()> = async {
            let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
            while let Some(row) = rows.try_next().await? {
                let schema_name = Self::get_str_with_null(&row, "schemaname")?;
                let view_name = Self::get_str_with_null(&row, "viewname")?;
                let definition = Self::get_str_with_null(&row, "definition")?;
                if self.filter_tb(&schema_name, &view_name) {
                    continue;
                }
                results.push(PgCreateViewStatement {
                    schema_name,
                    view_name,
                    kind: PgViewKind::MaterializedView,
                    definition,
                });
            }
            Ok(())
        }
        .await;

        if let Err(e) = matview_res {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("pg_matviews") && err_str.contains("does not exist") {
                // Some GaussDB deployments (ustore enabled) do not support materialized views and
                // don't expose `pg_matviews`. Treat as "no matviews" instead of failing the whole
                // struct extraction.
                log_warn!("skip materialized views (pg_matviews missing): {}", e);
            } else {
                return Err(e);
            }
        }

        Ok(results)
    }

    pub async fn get_create_rbac_statements(
        &mut self,
    ) -> anyhow::Result<Vec<PgCreateRbacStatement>> {
        let roles = self.get_roles().await?;
        let members = self.get_role_members().await?;
        let privileges = self.get_privileges().await?;
        Ok(vec![PgCreateRbacStatement {
            roles,
            members,
            privileges,
        }])
    }

    async fn get_schemas(&mut self, sch: &str) -> anyhow::Result<Vec<Schema>> {
        let (sch_filter, target_schemas) = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(Vec::new());
            }
            (
                format!("schema_name = '{}'", sch),
                HashSet::from([sch.to_string()]),
            )
        } else if !self.schemas.is_empty() {
            (
                format!("schema_name IN ({})", self.get_schemas_str()),
                self.schemas.clone(),
            )
        } else {
            return Ok(Vec::new());
        };

        let sql = format!(
            "SELECT schema_name 
            FROM information_schema.schemata
            WHERE {}",
            sch_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        let mut schemas = HashSet::new();
        while let Some(row) = rows.try_next().await? {
            let schema_name = Self::get_str_with_null(&row, "schema_name")?;
            schemas.insert(schema_name);
        }
        let filtered_schemas: Vec<String> = target_schemas
            .iter()
            .filter(|&s| !schemas.contains(s))
            .cloned()
            .collect();
        if !filtered_schemas.is_empty() {
            bail! {Error::StructError(format!(
                "schemas: {} not found",
                filtered_schemas.join(",")
            ))}
        } else {
            Ok(schemas.into_iter().map(|s| Schema { name: s }).collect())
        }
    }

    async fn get_sequences(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<Sequence>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("obj.sequence_schema='{}' AND tab.relname = '{}'", sch, tb)
            } else {
                format!("obj.sequence_schema = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            let schemas_str = &self.get_schemas_str();
            format!("obj.sequence_schema IN ({})", schemas_str)
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT obj.sequence_catalog,
                obj.sequence_schema,
                tab.relname AS table_name,
                obj.sequence_name,
                obj.data_type,
                obj.start_value,
                obj.minimum_value,
                obj.maximum_value,
                obj.increment,
                obj.cycle_option
            FROM information_schema.sequences obj
            JOIN pg_class AS seq
                ON (seq.relname = obj.sequence_name)
            JOIN pg_namespace ns
                ON (seq.relnamespace = ns.oid)
            JOIN pg_depend AS dep
                ON (seq.oid = dep.objid)
            JOIN pg_class AS tab
                ON (dep.refobjid = tab.oid)
            WHERE {} 
            AND ns.nspname = obj.sequence_schema 
            AND dep.deptype='a'",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let (sequence_schema, table_name, sequence_name): (String, String, String) = (
                Self::get_str_with_null(&row, "sequence_schema")?,
                Self::get_str_with_null(&row, "table_name")?,
                Self::get_str_with_null(&row, "sequence_name")?,
            );

            let sequence = Sequence {
                sequence_name,
                database_name: Self::get_str_with_null(&row, "sequence_catalog")?,
                schema_name: sequence_schema.clone(),
                data_type: Self::get_str_with_null(&row, "data_type")?,
                start_value: row.get("start_value"),
                increment: row.get("increment"),
                minimum_value: row.get("minimum_value"),
                maximum_value: row.get("maximum_value"),
                cycle_option: Self::get_str_with_null(&row, "cycle_option")?,
            };
            self.push_to_results(&mut results, &sequence_schema, &table_name, sequence);
        }

        Ok(results)
    }

    async fn get_independent_sequences(
        &mut self,
        sequence_names: &[String],
        table_schema: &str,
    ) -> anyhow::Result<Vec<Sequence>> {
        let filter_names: Vec<String> = sequence_names.iter().map(|i| format!("'{}'", i)).collect();
        let filter = format!("AND sequence_name IN ({})", filter_names.join(","));
        let sql = format!(
            "SELECT *
            FROM information_schema.sequences
            WHERE sequence_schema='{}' {}",
            table_schema, filter
        );

        let mut results = Vec::new();
        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let sequence = Sequence {
                sequence_name: Self::get_str_with_null(&row, "sequence_name")?,
                database_name: Self::get_str_with_null(&row, "sequence_catalog")?,
                schema_name: Self::get_str_with_null(&row, "sequence_schema")?,
                data_type: Self::get_str_with_null(&row, "data_type")?,
                start_value: row.get("start_value"),
                increment: row.get("increment"),
                minimum_value: row.get("minimum_value"),
                maximum_value: row.get("maximum_value"),
                cycle_option: Self::get_str_with_null(&row, "cycle_option")?,
            };
            results.push(sequence)
        }

        Ok(results)
    }

    async fn get_table_sequences(
        &mut self,
        table: &Table,
        sequences: &mut HashMap<(String, String), Vec<Sequence>>,
    ) -> anyhow::Result<Vec<Sequence>> {
        let mut table_sequences = self.get_result(
            sequences,
            &(table.schema_name.clone(), table.table_name.clone()),
        );

        let mut owned_sequence_names = HashSet::new();
        for sequence in table_sequences.iter() {
            owned_sequence_names.insert(sequence.sequence_name.clone());
        }

        let mut independent_sequence_names = Vec::new();
        for column in table.columns.iter() {
            if let Some(ColumnDefault::Literal(default_value)) = &column.column_default {
                let (schema, sequence_name) =
                    Self::get_sequence_name_by_default_value(default_value);
                // example, default_value is 'Standard'::text
                if sequence_name.is_empty() {
                    log_warn!(
                        "table: {}.{} has default value: {} for column: {}, not sequence",
                        table.schema_name,
                        table.table_name,
                        default_value,
                        column.column_name
                    );
                    continue;
                }

                // sequence and table should be in the same schema, otherwise we don't support
                if !schema.is_empty() && schema != table.schema_name {
                    log_error!(
                        "table: {}.{} is using sequence: {}.{} from a different schema",
                        table.schema_name,
                        table.table_name,
                        schema,
                        sequence_name
                    );
                    continue;
                }

                if owned_sequence_names.contains(&sequence_name) {
                    continue;
                }

                log_info!(
                    "table: {}.{} is using independent sequence: {}.{}",
                    table.schema_name,
                    table.table_name,
                    schema,
                    sequence_name
                );
                independent_sequence_names.push(sequence_name);
            }
        }

        if !independent_sequence_names.is_empty() {
            let independent_squences = self
                .get_independent_sequences(&independent_sequence_names, &table.schema_name)
                .await?;
            table_sequences.extend_from_slice(&independent_squences);
        }

        Ok(table_sequences)
    }

    async fn get_sequence_owners(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<SequenceOwner>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("ns.nspname='{}' AND tab.relname = '{}'", sch, tb)
            } else {
                format!("ns.nspname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("ns.nspname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT seq.relname,
                tab.relname AS table_name,
                attr.attname AS column_name,
                ns.nspname
            FROM pg_class AS seq
            JOIN pg_namespace ns
                ON (seq.relnamespace = ns.oid)
            JOIN pg_depend AS dep
                ON (seq.oid = dep.objid)
            JOIN pg_class AS tab
                ON (dep.refobjid = tab.oid)
            JOIN pg_attribute AS attr
                ON (attr.attnum = dep.refobjsubid AND attr.attrelid = dep.refobjid)
            WHERE dep.deptype='a'
                AND seq.relkind='S'
                AND {}",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);

        while let Some(row) = rows.try_next().await? {
            let (schema_name, table_name, seq_name): (String, String, String) = (
                Self::get_str_with_null(&row, "nspname")?,
                Self::get_str_with_null(&row, "table_name")?,
                Self::get_str_with_null(&row, "relname")?,
            );

            let sequence_owner = SequenceOwner {
                sequence_name: seq_name,
                database_name: String::new(),
                schema_name: schema_name.clone(),
                table_name: table_name.clone(),
                column_name: Self::get_str_with_null(&row, "column_name")?,
            };
            self.push_to_results(&mut results, &schema_name, &table_name, sequence_owner);
        }

        Ok(results)
    }

    async fn get_tables(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<BTreeMap<(String, String), Table>> {
        let mut results: BTreeMap<(String, String), Table> = BTreeMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("c.table_schema = '{}' AND c.table_name = '{}'", sch, tb)
            } else {
                format!("c.table_schema = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("c.table_schema IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT c.table_schema,
                c.table_name,
                c.column_name,
                c.data_type,
                c.udt_name,
                c.character_maximum_length,
                c.is_nullable,
                c.column_default,
                c.numeric_precision,
                c.numeric_scale,
                c.is_identity,
                c.identity_generation,
                c.ordinal_position
            FROM information_schema.columns c
            JOIN information_schema.tables t 
                ON c.table_schema = t.table_schema 
                AND c.table_name = t.table_name
            WHERE {} 
                AND t.table_type = 'BASE TABLE'
            ORDER BY c.table_schema, c.table_name, c.ordinal_position",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let (table_schema, table_name) = (
                Self::get_str_with_null(&row, "table_schema")?,
                Self::get_str_with_null(&row, "table_name")?,
            );

            if !self.schemas.contains(&table_schema) || self.filter_tb(&table_schema, &table_name) {
                continue;
            }

            let ordinal_position: i32 = row.try_get("ordinal_position")?;
            let is_identity = row.get("is_identity");
            let identity_generation = row.get("identity_generation");
            let generation_rule = Self::get_col_generation_rule(is_identity, identity_generation);
            let is_nullable = Self::get_str_with_null(&row, "is_nullable")?.to_lowercase() == "yes";
            let column_default = row
                .get::<Option<String>, _>("column_default")
                .map(ColumnDefault::Literal);
            let column = Column {
                column_name: Self::get_str_with_null(&row, "column_name")?,
                ordinal_position: ordinal_position as u32,
                column_default,
                is_nullable,
                generated: generation_rule,
                ..Default::default()
            };

            let key = (table_schema.clone(), table_name.clone());
            if let Some(table) = results.get_mut(&key) {
                table.columns.push(column);
            } else {
                results.insert(
                    key,
                    Table {
                        database_name: table_schema.clone(),
                        schema_name: table_schema,
                        table_name: table_name.clone(),
                        columns: vec![column],
                        ..Default::default()
                    },
                );
            }
        }

        // get column types
        for ((table_schema, table_name), table) in results.iter_mut() {
            let column_types = self.get_column_types(table_schema, table_name).await?;
            for column in table.columns.iter_mut() {
                column.column_type = column_types.get(&column.column_name).unwrap().to_owned();
            }
        }

        Ok(results)
    }

    async fn get_column_types(
        &mut self,
        schema: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<String, String>> {
        let fetcher = PgStructCheckFetcher {
            conn_pool: self.conn_pool.clone(),
            db_type: self
                .filter
                .as_ref()
                .map(|filter| filter.db_type.clone())
                .unwrap_or(DbType::Pg),
        };

        let oid = fetcher.get_oid(schema, tb).await?;

        if oid.is_empty() {
            anyhow::bail!(
                "Invalid OID: cannot be empty for schema: {} and table: {}",
                schema,
                tb
            );
        }

        let sql = format!(
            "SELECT a.attname AS column_name, 
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS column_type
            FROM pg_catalog.pg_attribute a
            WHERE a.attrelid = '{}' AND a.attnum > 0;",
            oid
        );

        let mut results = HashMap::new();
        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let column_name: String = Self::get_str_with_null(&row, "column_name")?;
            let column_type: String = Self::get_str_with_null(&row, "column_type")?;
            results.insert(column_name, column_type);
        }

        Ok(results)
    }

    async fn get_constraints(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<Constraint>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("nsp.nspname='{}' AND rel.relname = '{}'", sch, tb)
            } else {
                format!("nsp.nspname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("nsp.nspname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT nsp.nspname,
                rel.relname,
                con.conname AS constraint_name,
                con.contype AS constraint_type,
                pg_get_constraintdef(con.oid) AS constraint_definition
            FROM pg_catalog.pg_constraint con
            JOIN pg_catalog.pg_class rel
                ON rel.oid = con.conrelid
            JOIN pg_catalog.pg_namespace nsp
                ON nsp.oid = connamespace
            WHERE {} 
            ORDER BY nsp.nspname,rel.relname",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let table_name = Self::get_str_with_null(&row, "relname")?;
            let constraint_type = Self::get_with_null(&row, "constraint_type", ColType::Char)?;

            let constraint = Constraint {
                database_name: String::new(),
                schema_name: Self::get_str_with_null(&row, "nspname")?,
                table_name: table_name.clone(),
                constraint_name: Self::get_str_with_null(&row, "constraint_name")?,
                constraint_type: ConstraintType::from_str(&constraint_type, DbType::Pg),
                definition: Self::get_str_with_null(&row, "constraint_definition")?,
            };
            self.push_to_results(
                &mut results,
                &constraint.schema_name.clone(),
                &table_name,
                constraint,
            );
        }

        Ok(results)
    }

    async fn get_indexes(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<Index>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("schemaname='{}' AND tablename = '{}'", sch, tb)
            } else {
                format!("schemaname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            let schemas_str = self
                .schemas
                .iter()
                .map(|s| format!("'{}'", s))
                .collect::<Vec<_>>()
                .join(",");
            format!("schemaname IN ({})", schemas_str)
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT schemaname,
                tablename,
                indexdef,
                COALESCE(tablespace, 'pg_default') AS tablespace, indexname
            FROM pg_indexes
            WHERE {}",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let table_name = Self::get_str_with_null(&row, "tablename")?;
            let definition = Self::get_str_with_null(&row, "indexdef")?;

            let index = Index {
                schema_name: Self::get_str_with_null(&row, "schemaname")?,
                table_name: table_name.clone(),
                index_name: Self::get_str_with_null(&row, "indexname")?,
                index_kind: self.get_index_kind(&definition),
                table_space: Self::get_str_with_null(&row, "tablespace")?,
                definition,
                ..Default::default()
            };
            self.push_to_results(&mut results, &index.schema_name.clone(), &table_name, index);
        }

        Ok(results)
    }

    async fn get_table_comments(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<Comment>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("n.nspname='{}' AND c.relname = '{}'", sch, tb)
            } else {
                format!("n.nspname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("n.nspname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT n.nspname,
                c.relname,
                d.description
            FROM pg_class c
            LEFT JOIN pg_namespace n
                ON n.oid = c.relnamespace
            LEFT JOIN pg_description d
                ON c.oid = d.objoid  AND d.objsubid = 0
            WHERE {} 
            AND d.description IS NOT null",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let (schema_name, table_name): (String, String) = (
                Self::get_str_with_null(&row, "nspname")?,
                Self::get_str_with_null(&row, "relname")?,
            );

            let comment = Comment {
                comment_type: CommentType::Table,
                database_name: String::new(),
                schema_name: schema_name.clone(),
                table_name: table_name.clone(),
                column_name: String::new(),
                comment: Self::get_str_with_null(&row, "description")?,
            };
            self.push_to_results(&mut results, &schema_name, &table_name, comment);
        }

        Ok(results)
    }

    async fn get_column_comments(
        &mut self,
        sch: &str,
        tb: &str,
    ) -> anyhow::Result<HashMap<(String, String), Vec<Comment>>> {
        let mut results = HashMap::new();

        let tb_filter = if !sch.is_empty() {
            if !self.schemas.contains(sch) {
                return Ok(results);
            }
            if !tb.is_empty() {
                format!("n.nspname='{}' AND c.relname = '{}'", sch, tb)
            } else {
                format!("n.nspname = '{}'", sch)
            }
        } else if !self.schemas.is_empty() {
            format!("n.nspname IN ({})", self.get_schemas_str())
        } else {
            return Ok(results);
        };

        let sql = format!(
            "SELECT n.nspname,
                c.relname,
                col_description(a.attrelid, a.attnum) as comment,
                format_type(a.atttypid, a.atttypmod)as type,
                a.attname AS name,
                a.attnotnull AS notnull
            FROM pg_class c
            LEFT JOIN pg_attribute a
                ON a.attrelid =c.oid
            LEFT JOIN pg_namespace n
                ON n.oid = c.relnamespace
            WHERE {} 
                AND a.attnum >0
                AND col_description(a.attrelid, a.attnum) is NOT null",
            tb_filter
        );

        let mut rows = sqlx::query(&sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let (schema_name, table_name, column_name) = (
                Self::get_str_with_null(&row, "nspname")?,
                Self::get_str_with_null(&row, "relname")?,
                Self::get_str_with_null(&row, "name")?,
            );

            let comment = Comment {
                comment_type: CommentType::Column,
                database_name: String::new(),
                schema_name: schema_name.clone(),
                table_name: table_name.clone(),
                column_name,
                comment: Self::get_str_with_null(&row, "comment")?,
            };
            self.push_to_results(&mut results, &schema_name, &table_name, comment);
        }

        Ok(results)
    }

    // temporarily not migrating superuser role
    async fn get_roles(&mut self) -> anyhow::Result<Vec<PgRole>> {
        let sql = "SELECT a.rolname, a.rolpassword, a.rolsuper, a.rolinherit, a.rolcreaterole, 
                a.rolcreatedb, a.rolcanlogin, a.rolreplication, a.rolbypassrls, a.rolconnlimit, 
                a.rolvaliduntil, r.rolconfig 
            FROM pg_authid a
            JOIN pg_roles r ON a.oid = r.oid
            WHERE a.rolname NOT LIKE 'pg_%' 
            AND a.rolname NOT IN ('postgres') 
            AND a.oid >= 16384
            AND r.rolsuper = false
        ";

        let mut results = Vec::new();
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);

        while let Some(row) = rows.try_next().await? {
            let name = Self::get_str_with_null(&row, "rolname")?;
            let password = Self::get_str_with_null(&row, "rolpassword")?;

            let rol_conn_limit: i32 = row.try_get("rolconnlimit").unwrap_or(-1);
            let rol_valid_until: Option<String> = row.try_get("rolvaliduntil").unwrap_or(None);

            let rol_configs: Vec<String> = match row.try_get::<Option<Vec<String>>, _>("rolconfig")
            {
                Ok(Some(configs)) => configs,
                _ => Vec::new(),
            };

            let role = PgRole {
                name,
                password,
                rol_super: row.try_get("rolsuper").unwrap_or(false),
                rol_inherit: row.try_get("rolinherit").unwrap_or(false),
                rol_createrole: row.try_get("rolcreaterole").unwrap_or(false),
                rol_createdb: row.try_get("rolcreatedb").unwrap_or(false),
                rol_can_login: row.try_get("rolcanlogin").unwrap_or(false),
                rol_replication: row.try_get("rolreplication").unwrap_or(false),
                rol_by_passrls: row.try_get("rolbypassrls").unwrap_or(false),
                rol_conn_limit: rol_conn_limit.to_string(),
                rol_valid_until: rol_valid_until.unwrap_or_default(),
                rol_configs,
            };

            results.push(role);
        }

        Ok(results)
    }

    async fn get_role_members(&mut self) -> anyhow::Result<Vec<PgRoleMember>> {
        let sql = "SELECT 
            m.roleid::regrole::text AS group_name,
            m.member::regrole::text AS member_name,
            admin_option
        FROM 
            pg_auth_members m 
        WHERE 
            m.member >= 16384";

        let mut results = Vec::new();
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);

        while let Some(row) = rows.try_next().await? {
            let role = match row.try_get::<String, _>("group_name") {
                Ok(name) => name,
                Err(_) => continue,
            };
            let member = match row.try_get::<String, _>("member_name") {
                Ok(name) => name,
                Err(_) => continue,
            };
            let admin_option = row.try_get::<bool, _>("admin_option").unwrap_or(false);

            results.push(PgRoleMember {
                role,
                member,
                admin_option,
            });
        }

        Ok(results)
    }

    async fn get_privileges(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        let mut results = Vec::new();
        results.extend(self.get_schema_privilege().await?);
        results.extend(self.get_table_privilege().await?);
        results.extend(self.get_column_privilege().await?);
        results.extend(self.get_routine_privilege().await?);
        results.extend(self.get_sequence_privilege().await?);
        Ok(results)
    }

    async fn get_routine_privilege(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        // Prefer `prokind` (PG >= 11) to distinguish function vs procedure. Fallback when missing.
        let sql_with_prokind = "SELECT
                n.nspname AS schema_name,
                p.proname AS routine_name,
                p.prokind::text AS prokind,
                pg_get_function_identity_arguments(p.oid) AS identity_args,
                acl.grantee::regrole::text AS grantee,
                string_agg(acl.privilege_type, ',') AS privileges,
                bool_or(acl.is_grantable) AS is_grantable
            FROM
                pg_proc p
                JOIN pg_namespace n ON n.oid = p.pronamespace
                JOIN LATERAL aclexplode(p.proacl) AS acl ON true
            WHERE
                p.proacl IS NOT NULL AND
                acl.grantee != 0 AND
                acl.grantee::regrole::text NOT LIKE 'pg_%' AND
                acl.grantee::regrole::text NOT IN ('postgres', 'PUBLIC')
            GROUP BY
                n.nspname,
                p.proname,
                p.prokind,
                pg_get_function_identity_arguments(p.oid),
                acl.grantee::regrole::text";

        let sql_without_prokind = "SELECT
                n.nspname AS schema_name,
                p.proname AS routine_name,
                pg_get_function_identity_arguments(p.oid) AS identity_args,
                acl.grantee::regrole::text AS grantee,
                string_agg(acl.privilege_type, ',') AS privileges,
                bool_or(acl.is_grantable) AS is_grantable,
                pg_get_functiondef(p.oid) AS create_sql
            FROM
                pg_proc p
                JOIN pg_namespace n ON n.oid = p.pronamespace
                JOIN LATERAL aclexplode(p.proacl) AS acl ON true
            WHERE
                p.proacl IS NOT NULL AND
                acl.grantee != 0 AND
                acl.grantee::regrole::text NOT LIKE 'pg_%' AND
                acl.grantee::regrole::text NOT IN ('postgres', 'PUBLIC')
            GROUP BY
                n.nspname,
                p.proname,
                pg_get_function_identity_arguments(p.oid),
                acl.grantee::regrole::text,
                pg_get_functiondef(p.oid)";

        let mut results = Vec::new();
        let with_prokind_res: anyhow::Result<()> = async {
            let mut rows = sqlx::query(sql_with_prokind).fetch(&self.conn_pool);
            while let Some(row) = rows.try_next().await? {
                let schema_name = Self::get_str_with_null(&row, "schema_name")?;
                let routine_name = Self::get_str_with_null(&row, "routine_name")?;
                let prokind = Self::get_str_with_null(&row, "prokind")?;
                let identity_args = Self::get_str_with_null(&row, "identity_args")?;
                let grantee = Self::get_str_with_null(&row, "grantee")?;
                let privileges = Self::get_str_with_null(&row, "privileges")?;
                let is_grantable = row.try_get::<bool, _>("is_grantable").unwrap_or(false);

                if self.filter_tb(&schema_name, &routine_name) {
                    continue;
                }

                let obj_kind = match prokind.as_str() {
                    "p" => "PROCEDURE",
                    _ => "FUNCTION",
                };
                let grant_command = Self::build_routine_grant_sql(
                    &privileges,
                    obj_kind,
                    &schema_name,
                    &routine_name,
                    &identity_args,
                    &grantee,
                    is_grantable,
                );
                results.push(PgPrivilege {
                    origin: grant_command,
                });
            }
            Ok(())
        }
        .await;

        match with_prokind_res {
            Ok(()) => Ok(results),
            Err(e) => {
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("prokind") {
                    log_warn!(
                        "routine grants fallback (pg_proc.prokind unavailable), error: {}",
                        e
                    );
                    results.clear();
                    let mut rows = sqlx::query(sql_without_prokind).fetch(&self.conn_pool);
                    while let Some(row) = rows.try_next().await? {
                        let schema_name = Self::get_str_with_null(&row, "schema_name")?;
                        let routine_name = Self::get_str_with_null(&row, "routine_name")?;
                        let identity_args = Self::get_str_with_null(&row, "identity_args")?;
                        let grantee = Self::get_str_with_null(&row, "grantee")?;
                        let privileges = Self::get_str_with_null(&row, "privileges")?;
                        let is_grantable = row.try_get::<bool, _>("is_grantable").unwrap_or(false);
                        let create_sql = Self::get_str_with_null(&row, "create_sql")?;

                        if self.filter_tb(&schema_name, &routine_name) {
                            continue;
                        }

                        let obj_kind = Self::infer_routine_obj_kind_from_create_sql(&create_sql);
                        let grant_command = Self::build_routine_grant_sql(
                            &privileges,
                            obj_kind,
                            &schema_name,
                            &routine_name,
                            &identity_args,
                            &grantee,
                            is_grantable,
                        );
                        results.push(PgPrivilege {
                            origin: grant_command,
                        });
                    }
                    Ok(results)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn infer_routine_obj_kind_from_create_sql(create_sql: &str) -> &'static str {
        let header = create_sql.trim_start().to_uppercase();
        if header.starts_with("CREATE OR REPLACE PROCEDURE")
            || header.starts_with("CREATE PROCEDURE")
        {
            return "PROCEDURE";
        }
        "FUNCTION"
    }

    fn build_routine_grant_sql(
        privileges: &str,
        obj_kind: &str,
        schema_name: &str,
        routine_name: &str,
        identity_args: &str,
        grantee: &str,
        is_grantable: bool,
    ) -> String {
        let grant_option = if is_grantable {
            " WITH GRANT OPTION"
        } else {
            ""
        };
        format!(
            "GRANT {} ON {} \"{}\".\"{}\"({}) TO \"{}\"{}",
            privileges, obj_kind, schema_name, routine_name, identity_args, grantee, grant_option
        )
    }

    async fn get_schema_privilege(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        let sql = "SELECT
              n.nspname AS schema_name,
              'GRANT ' || 
              string_agg(acl.privilege_type, ',') || 
              ' ON SCHEMA ' || quote_ident(n.nspname) || 
              ' TO ' || quote_ident(acl.grantee::regrole::text) AS grant_command
            FROM 
              pg_namespace n,
              LATERAL aclexplode(n.nspacl) AS acl
            WHERE 
              n.nspacl IS NOT NULL AND
              acl.grantee != 0 AND
              acl.grantee::regrole::text NOT LIKE 'pg_%' AND 
              acl.grantee::regrole::text NOT IN ('postgres', 'PUBLIC')
            GROUP BY
              n.nspname, acl.grantee::regrole::text";

        let mut results = Vec::new();
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let grant_command = row.get::<String, _>("grant_command");
            let schema_name = Self::get_str_with_null(&row, "schema_name")?;

            if self.filter_schema(schema_name.as_str()) {
                continue;
            }

            results.push(PgPrivilege {
                origin: grant_command,
            });
        }

        Ok(results)
    }

    async fn get_table_privilege(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        let sql = "SELECT 
                table_schema,
                table_name,
                'GRANT ' || 
                array_to_string(array_agg(privilege_type), ',') || 
                ' ON ' || quote_ident(table_schema) || '.' || quote_ident(table_name) || 
                ' TO ' || quote_ident(grantee) ||
                CASE 
                    WHEN is_grantable = 'YES' THEN ' WITH GRANT OPTION'
                    ELSE ''
                END AS grant_command
            FROM 
                information_schema.role_table_grants 
            WHERE 
                grantee NOT LIKE 'pg_%' AND 
                grantee NOT IN ('postgres', 'PUBLIC')
            GROUP BY 
                table_schema, table_name, grantee, is_grantable";

        let mut results = Vec::new();
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);
        while let Some(row) = rows.try_next().await? {
            let schema_name = Self::get_str_with_null(&row, "table_schema")?;
            let table_name = Self::get_str_with_null(&row, "table_name")?;
            let grant_command = Self::get_str_with_null(&row, "grant_command")?;

            if self.filter_tb(&schema_name, &table_name) {
                continue;
            }

            results.push(PgPrivilege {
                origin: grant_command,
            });
        }

        Ok(results)
    }

    #[allow(clippy::type_complexity)]
    async fn get_column_privilege(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        let sql = "SELECT 
                rcg.table_schema,
                rcg.table_name,
                rcg.column_name,
                rcg.privilege_type,
                rcg.grantee,
                rcg.is_grantable
            FROM 
                information_schema.role_column_grants rcg
            LEFT JOIN information_schema.role_table_grants rtg ON 
                rtg.grantee = rcg.grantee AND 
                rtg.table_name = rcg.table_name AND 
                rtg.table_schema = rcg.table_schema AND
                rtg.table_catalog = rcg.table_catalog AND
                rtg.privilege_type = rcg.privilege_type
            WHERE 
                rcg.grantee NOT LIKE 'pg_%' AND 
                rcg.grantee NOT IN ('postgres', 'PUBLIC') AND
                rtg.grantee IS NULL
            ORDER BY
                rcg.table_schema, rcg.table_name, rcg.grantee
            ";

        let mut results = Vec::new();
        let mut rows = sqlx::query(sql).fetch(&self.conn_pool);

        // key: (schema, table, grantee, is_grantable)
        // value: (privilege_type -> columns_set)
        let mut privilege_data: HashMap<
            (String, String, String, String),
            HashMap<String, HashSet<String>>,
        > = HashMap::new();

        while let Some(row) = rows.try_next().await? {
            let schema_name = Self::get_str_with_null(&row, "table_schema")?;
            let table_name = Self::get_str_with_null(&row, "table_name")?;

            if self.filter_tb(&schema_name, &table_name) {
                continue;
            }

            let column_name = Self::get_str_with_null(&row, "column_name")?;
            let privilege_type = Self::get_str_with_null(&row, "privilege_type")?;
            let grantee = Self::get_str_with_null(&row, "grantee")?;
            let is_grantable = Self::get_str_with_null(&row, "is_grantable")?;

            let key = (schema_name, table_name, grantee, is_grantable);

            let privilege_map = privilege_data.entry(key).or_default();

            let columns = privilege_map.entry(privilege_type).or_default();

            columns.insert(column_name);
        }

        for ((schema, table, grantee, is_grantable), privilege_map) in privilege_data {
            for (privilege_type, columns) in privilege_map {
                let mut columns_vec: Vec<String> = columns.into_iter().collect();
                columns_vec.sort();

                let quoted_columns = columns_vec
                    .iter()
                    .map(|col| format!("\"{}\"", col))
                    .collect::<Vec<_>>()
                    .join(", ");

                let grant_option = if is_grantable == "YES" {
                    " WITH GRANT OPTION"
                } else {
                    ""
                };

                // Format: GRANT SELECT (column1, column2) ON table_name TO role_name [WITH GRANT OPTION]
                let grant_command = format!(
                    "GRANT {} ({}) ON \"{}\".\"{}\" TO \"{}\"{}",
                    privilege_type, quoted_columns, schema, table, grantee, grant_option
                );

                results.push(PgPrivilege {
                    origin: grant_command,
                });
            }
        }

        Ok(results)
    }

    async fn get_sequence_privilege(&mut self) -> anyhow::Result<Vec<PgPrivilege>> {
        let mut results = Vec::new();

        let tables = self.get_tables("", "").await?;
        let mut sequence_map: HashMap<String, HashSet<String>> = HashMap::new();
        for table in tables.values() {
            for column in &table.columns {
                if let Some(ColumnDefault::Literal(default_value)) = &column.column_default {
                    let (schema, sequence_name) =
                        Self::get_sequence_name_by_default_value(default_value);
                    if schema.is_empty() {
                        continue;
                    }
                    if !sequence_name.is_empty() {
                        let sequences = sequence_map.entry(schema).or_default();
                        sequences.insert(sequence_name);
                    }
                }
            }
        }
        if sequence_map.is_empty() {
            return Ok(results);
        }

        let sql = r#"
            SELECT grantor, grantee, object_catalog, object_schema, object_name, 
                   object_type, privilege_type, is_grantable
            FROM information_schema.role_usage_grants 
            WHERE object_type = 'SEQUENCE'
              AND grantee NOT LIKE 'pg\_%' 
              AND grantee <> 'postgres'
        "#;

        let rows = sqlx::query(sql).fetch_all(&self.conn_pool).await?;
        let mut privilege_data: HashMap<(String, String, String, String), HashSet<String>> =
            HashMap::new();

        for row in rows {
            let grantee = Self::get_str_with_null(&row, "grantee")?;
            let schema_name = Self::get_str_with_null(&row, "object_schema")?;
            let sequence_name = Self::get_str_with_null(&row, "object_name")?;
            let privilege_type = Self::get_str_with_null(&row, "privilege_type")?;
            let is_grantable = Self::get_str_with_null(&row, "is_grantable")?;

            if !sequence_map.contains_key(&schema_name) {
                continue;
            }

            let key = (schema_name, sequence_name, grantee, is_grantable);

            let privileges = privilege_data.entry(key).or_default();
            privileges.insert(privilege_type);
        }

        for ((schema, sequence, grantee, is_grantable), privileges) in privilege_data {
            let mut privileges_vec: Vec<String> = privileges.into_iter().collect();
            privileges_vec.sort();

            let privileges_str = privileges_vec.join(", ");

            let grant_option = if is_grantable == "YES" {
                " WITH GRANT OPTION"
            } else {
                ""
            };

            // format: GRANT USAGE, SELECT ON SEQUENCE schema.sequence_name TO role_name [WITH GRANT OPTION]
            let grant_command = format!(
                "GRANT {} ON SEQUENCE \"{}\".\"{}\" TO \"{}\"{}",
                privileges_str, schema, sequence, grantee, grant_option
            );

            results.push(PgPrivilege {
                origin: grant_command,
            });
        }

        Ok(results)
    }

    fn get_index_kind(&self, definition: &str) -> IndexKind {
        if definition.starts_with("CREATE UNIQUE INDEX") {
            IndexKind::Unique
        } else {
            IndexKind::Unknown
        }
    }

    fn get_str_with_null(row: &PgRow, col_name: &str) -> anyhow::Result<String> {
        Self::get_with_null(row, col_name, ColType::Text)
    }

    fn get_with_null(row: &PgRow, col_name: &str, col_type: ColType) -> anyhow::Result<String> {
        let mut str_val = String::new();
        match col_type {
            ColType::Text => {
                let str_val_option: Option<String> = row.get(col_name);
                if let Some(s) = str_val_option {
                    str_val = s
                }
            }
            ColType::Char => {
                let char_val: i8 = row.get(col_name);
                str_val = char_val.to_string();
            }
        }
        Ok(str_val)
    }

    fn get_col_generation_rule(
        is_identity: Option<String>,
        identity_generation: Option<String>,
    ) -> Option<String> {
        if let Some(i) = is_identity {
            if i.to_lowercase() == "yes" && identity_generation.is_some() {
                return identity_generation;
            }
        }
        None
    }

    fn get_sequence_name_by_default_value(default_value: &str) -> (String, String) {
        // SELECT table_schema,
        //     table_name,
        //     column_name,
        //     column_default
        // FROM information_schema.columns
        // WHERE table_schema ='public' and table_name='sequence_test_4';

        // case 1: when search_path is the same with sequence schema, column_default be like:
        // nextval('"aaaaaaadefdfd.dsds::er3\ddd"'::regclass)

        // case 2: when search_path is not the same with sequence schema, column_default be like:
        // nextval('public."aaaaaaadefdfd.dsds::er3\ddd"'::regclass)
        // nextval('"ddddd.ddddddds**"."aaaaaaadefdfd.dsds::er3\ddd"'::regclass)

        let mut value = default_value.trim();
        if !value.starts_with("nextval(") {
            return (String::new(), String::new());
        }

        value = value
            .trim_start_matches("nextval(")
            .trim_start_matches('\'')
            .trim_end_matches(')')
            // ::regclass may not exists
            .trim_end_matches("::regclass")
            .trim_end_matches('\'');

        let escape_pair = SqlUtil::get_escape_pairs(&DbType::Pg)[0];
        if let Ok(tokens) = ConfigTokenParser::parse_config(value, &DbType::Pg, &['.'], None) {
            if tokens.len() == 1 {
                return (String::new(), SqlUtil::unescape(&tokens[0], &escape_pair));
            } else if tokens.len() == 2 {
                return (
                    SqlUtil::unescape(&tokens[0], &escape_pair),
                    SqlUtil::unescape(&tokens[1], &escape_pair),
                );
            }
        }
        (String::new(), String::new())
    }

    fn filter_tb(&self, schema: &str, tb: &str) -> bool {
        if let Some(filter) = &self.filter {
            return filter.filter_tb(schema, tb);
        }
        false
    }

    fn filter_schema(&self, schema: &str) -> bool {
        if let Some(filter) = &self.filter {
            return filter.filter_schema(schema);
        }
        false
    }

    fn push_to_results<T>(
        &mut self,
        results: &mut HashMap<(String, String), Vec<T>>,
        schema_name: &str,
        table_name: &str,
        item: T,
    ) {
        if !self.schemas.contains(schema_name) || self.filter_tb(schema_name, table_name) {
            return;
        }

        let key = (schema_name.into(), table_name.into());
        if let Some(exists) = results.get_mut(&key) {
            exists.push(item);
        } else {
            results.insert(key, vec![item]);
        }
    }

    fn get_result<T>(
        &self,
        results: &mut HashMap<(String, String), Vec<T>>,
        key: &(String, String),
    ) -> Vec<T> {
        results.remove(key).unwrap_or_default()
    }

    fn get_schemas_str(&self) -> String {
        self.schemas
            .iter()
            .map(|s| format!("'{}'", s))
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[cfg(test)]
mod tests {
    use crate::meta_fetcher::pg::pg_struct_fetcher::PgStructFetcher;

    #[test]
    fn get_sequence_name_by_default_value_test() {
        let default_values = [
            r#"('"aaaaaaadefdfd.dsds::er3\ddd"'::regclass)"#,
            r#"nextval('aaaaaaaaaa'::regclass)"#,
            r#"nextval('public.aaaaaaaaaa'::regclass)"#,
            r#"nextval('"aaaaaaadefdfd.dsds::er3\ddd"'::regclass)"#,
            r#"nextval('public."aaaaaaadefdfd.dsds::er3\ddd"'::regclass)"#,
            r#"nextval('"ddddd.ddddddds**"."aaaaaaadefdfd.dsds::er3\ddd"'::regclass)"#,
            r#"nextval('"aaaaaaadefdfd.dsds::er3\ddd"')"#,
            r#"nextval('public."aaaaaaadefdfd.dsds::er3\ddd"')"#,
            r#"nextval('"ddddd.ddddddds**"."aaaaaaadefdfd.dsds::er3\ddd"')"#,
        ];

        let expect_sequences = vec![
            ("", ""),
            ("", "aaaaaaaaaa"),
            ("public", "aaaaaaaaaa"),
            ("", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
            ("public", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
            ("ddddd.ddddddds**", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
            ("", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
            ("public", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
            ("ddddd.ddddddds**", r#"aaaaaaadefdfd.dsds::er3\ddd"#),
        ];

        for i in 0..default_values.len() {
            let (schema, sequence) =
                PgStructFetcher::get_sequence_name_by_default_value(default_values[i]);
            assert_eq!((schema.as_str(), sequence.as_str()), expect_sequences[i]);
        }
    }

    #[test]
    fn build_routine_grant_sql_handles_empty_args() {
        let sql = PgStructFetcher::build_routine_grant_sql(
            "EXECUTE", "FUNCTION", "public", "f1", "", "role1", false,
        );
        assert_eq!(
            sql,
            "GRANT EXECUTE ON FUNCTION \"public\".\"f1\"() TO \"role1\""
        );
    }

    #[test]
    fn build_routine_grant_sql_appends_grant_option() {
        let sql = PgStructFetcher::build_routine_grant_sql(
            "EXECUTE",
            "PROCEDURE",
            "public",
            "p1",
            "integer, text",
            "role1",
            true,
        );
        assert_eq!(
            sql,
            "GRANT EXECUTE ON PROCEDURE \"public\".\"p1\"(integer, text) TO \"role1\" WITH GRANT OPTION"
        );
    }

    #[test]
    fn infer_routine_obj_kind_from_create_sql_works() {
        assert_eq!(
            PgStructFetcher::infer_routine_obj_kind_from_create_sql(
                "CREATE OR REPLACE PROCEDURE public.p1() LANGUAGE plpgsql AS $$ BEGIN END; $$;"
            ),
            "PROCEDURE"
        );
        assert_eq!(
            PgStructFetcher::infer_routine_obj_kind_from_create_sql(
                "CREATE OR REPLACE FUNCTION public.f1() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;"
            ),
            "FUNCTION"
        );
    }
}
