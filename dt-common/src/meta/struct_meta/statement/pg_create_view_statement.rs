use crate::rdb_filter::RdbFilter;

use crate::meta::struct_meta::structure::structure_type::StructureType;

#[derive(Debug, Clone, PartialEq)]
pub enum PgViewKind {
    View,
    MaterializedView,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PgCreateViewStatement {
    pub schema_name: String,
    pub view_name: String,
    pub kind: PgViewKind,
    /// The view query body (typically a SELECT ...). Should not include trailing ';'.
    pub definition: String,
}

impl PgCreateViewStatement {
    pub fn route(&mut self, dst_schema: &str, dst_view: &str) {
        self.schema_name = dst_schema.to_string();
        self.view_name = dst_view.to_string();
    }

    pub fn to_sqls(&self, filter: &RdbFilter) -> anyhow::Result<Vec<(String, String)>> {
        let mut sqls = Vec::new();
        if filter.filter_structure(&StructureType::View) {
            return Ok(sqls);
        }

        // Views are schema-bound objects; also respect table-level filters if configured.
        if filter.filter_tb(&self.schema_name, &self.view_name) {
            return Ok(sqls);
        }

        let def = self.definition.trim().trim_end_matches(';').trim();
        let key_prefix = match self.kind {
            PgViewKind::View => "view",
            PgViewKind::MaterializedView => "matview",
        };
        let key = format!("{}.{}.{}", key_prefix, self.schema_name, self.view_name);

        let sql = match self.kind {
            PgViewKind::View => format!(
                r#"CREATE OR REPLACE VIEW "{}"."{}" AS {}"#,
                self.schema_name, self.view_name, def
            ),
            PgViewKind::MaterializedView => {
                // Materialized view: create definition only, without data. If already exists, skip.
                //
                // NOTE: we intentionally do not attempt to replace/rebuild existing matviews
                // because it can be expensive and may break dependencies.
                format!(
                    r#"DO $ape_dts$
DECLARE
  errm text;
BEGIN
  BEGIN
    EXECUTE $ape_dts_mv_with_no_data$CREATE MATERIALIZED VIEW "{schema}"."{name}" AS {def} WITH NO DATA$ape_dts_mv_with_no_data$;
  EXCEPTION
    WHEN duplicate_table THEN NULL;
    WHEN OTHERS THEN
      GET STACKED DIAGNOSTICS errm = MESSAGE_TEXT;
      IF errm ILIKE '%WITH NO DATA%' THEN
        BEGIN
          EXECUTE $ape_dts_mv_plain$CREATE MATERIALIZED VIEW "{schema}"."{name}" AS {def}$ape_dts_mv_plain$;
        EXCEPTION
          WHEN duplicate_table THEN NULL;
          WHEN OTHERS THEN
            GET STACKED DIAGNOSTICS errm = MESSAGE_TEXT;
            IF errm ILIKE '%materialized view%not supported%' THEN
              EXECUTE $ape_dts_mv_view_fallback$CREATE OR REPLACE VIEW "{schema}"."{name}" AS {def}$ape_dts_mv_view_fallback$;
            ELSE
              RAISE;
            END IF;
        END;
      ELSIF errm ILIKE '%materialized view%not supported%' THEN
        EXECUTE $ape_dts_mv_view_fallback$CREATE OR REPLACE VIEW "{schema}"."{name}" AS {def}$ape_dts_mv_view_fallback$;
      ELSE
        RAISE;
      END IF;
  END;
END
$ape_dts$;"#,
                    schema = self.schema_name,
                    name = self.view_name,
                    def = def
                )
            }
        };

        sqls.push((key, sql));
        Ok(sqls)
    }
}
