use anyhow::{bail, Context};

use crate::oracle::OracleSqlPlusClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogMinerCursor {
    pub(crate) scn: u64,
    pub(crate) rs_id: String,
    pub(crate) ssn: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct LogMinerRow {
    pub(crate) scn: u64,
    pub(crate) rs_id: String,
    pub(crate) ssn: u64,
    pub(crate) operation: String,
    pub(crate) schema: String,
    pub(crate) tb: String,
    pub(crate) sql_redo: String,
    pub(crate) sql_undo: String,
}

impl LogMinerCursor {
    pub(crate) fn new(scn: u64) -> Self {
        Self {
            scn,
            rs_id: String::new(),
            ssn: 0,
        }
    }

    pub(crate) fn from_row(row: &LogMinerRow) -> Self {
        Self {
            scn: row.scn,
            rs_id: row.rs_id.clone(),
            ssn: row.ssn,
        }
    }

    pub(crate) fn without_row_position(&self) -> Self {
        Self::new(self.scn)
    }

    pub(crate) fn has_row_position(&self) -> bool {
        !self.rs_id.is_empty()
    }
}

pub(crate) async fn current_scn(client: &OracleSqlPlusClient) -> anyhow::Result<u64> {
    let lines = client
        .query_lines("SELECT CURRENT_SCN FROM V$DATABASE")
        .await?;
    let first = lines
        .first()
        .context("oracle current_scn query returned no rows")?;
    first
        .trim()
        .parse::<u64>()
        .with_context(|| format!("invalid CURRENT_SCN: {}", first))
}

pub(crate) async fn redo_log_files(client: &OracleSqlPlusClient) -> anyhow::Result<Vec<String>> {
    let mut lines = client
        .query_lines("SELECT MEMBER FROM V$LOGFILE ORDER BY MEMBER")
        .await?;
    lines.retain(|s| !s.trim().is_empty() && s.trim() != "<NULL>");
    lines.sort();
    lines.dedup();
    if lines.is_empty() {
        bail!("oracle logminer redo log files not found from V$LOGFILE");
    }
    Ok(lines)
}

pub(crate) async fn fetch_logmnr_rows_in_range(
    client: &OracleSqlPlusClient,
    redo_logs: &[String],
    cursor: &LogMinerCursor,
    end_scn: u64,
    captured: &[(String, String)],
    limit: usize,
) -> anyhow::Result<Vec<LogMinerRow>> {
    if cursor.scn == 0 || end_scn == 0 || end_scn < cursor.scn {
        bail!(
            "invalid logminer scn range: cursor_scn={}, end_scn={}",
            cursor.scn,
            end_scn
        );
    }
    if redo_logs.is_empty() {
        bail!("oracle logminer redo_logs empty");
    }

    let pairs = build_seg_owner_table_predicate(captured)?;
    let cursor_predicate = build_cursor_predicate(cursor);
    let start_scn = logminer_start_scn(cursor)?;

    // NOTE: LogMiner state is session-scoped. Because `OracleSqlPlusClient` runs `sqlplus` as a
    // fresh process per call, we must do START_LOGMNR -> SELECT -> END_LOGMNR in the same script.
    let mut script = String::new();
    script.push_str("SET TERMOUT OFF\n");
    script.push_str("BEGIN\n");
    for (idx, path) in redo_logs.iter().enumerate() {
        let opt = if idx == 0 {
            "DBMS_LOGMNR.NEW"
        } else {
            "DBMS_LOGMNR.ADDFILE"
        };
        script.push_str(&format!(
            "  DBMS_LOGMNR.ADD_LOGFILE(LOGFILENAME => '{}', OPTIONS => {});\n",
            escape_sql_string(path),
            opt
        ));
    }
    script.push_str(&format!(
        "  DBMS_LOGMNR.START_LOGMNR(STARTSCN => {}, ENDSCN => {}, OPTIONS => DBMS_LOGMNR.DICT_FROM_ONLINE_CATALOG + DBMS_LOGMNR.COMMITTED_DATA_ONLY);\n",
        start_scn, end_scn
    ));
    script.push_str("END;\n/\n");
    script.push_str("SET TERMOUT ON\n");
    script.push_str(&format!(
        "SELECT scn, rs_id, ssn, operation, seg_owner, table_name, sql_redo, sql_undo FROM (SELECT scn, rs_id, ssn, operation, seg_owner, table_name, sql_redo, sql_undo FROM V$LOGMNR_CONTENTS WHERE operation IN ('INSERT','UPDATE','DELETE') AND ({cursor_predicate}) AND scn <= {end_scn} AND ({pairs}) ORDER BY scn ASC, rs_id ASC, ssn ASC) WHERE ROWNUM <= {limit};\n",
        cursor_predicate = cursor_predicate,
        end_scn = end_scn,
        pairs = pairs,
        limit = limit
    ));
    script.push_str("SET TERMOUT OFF\n");
    script.push_str("BEGIN DBMS_LOGMNR.END_LOGMNR; END;\n/\n");

    let lines = client.query_lines(&script).await?;
    let mut out = Vec::new();
    for line in lines {
        if let Some(row) = LogMinerRow::try_from_line(&line)? {
            out.push(row);
        }
    }
    Ok(out)
}

impl LogMinerRow {
    fn try_from_line(line: &str) -> anyhow::Result<Option<Self>> {
        if line.trim().is_empty() {
            return Ok(None);
        }
        if line.matches('|').count() < 7 {
            return Ok(None);
        }
        Ok(Some(Self::from_line(line)?))
    }

    fn from_line(line: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 8 {
            bail!(
                "oracle logminer row parse failed (expected 8 cols), line={}",
                line
            );
        }

        Ok(Self {
            scn: parts[0]
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid scn in logminer line: {}", line))?,
            rs_id: parts[1].trim().to_string(),
            ssn: parts[2]
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid ssn in logminer line: {}", line))?,
            operation: parts[3].trim().to_uppercase(),
            schema: parts[4].trim().to_uppercase(),
            tb: parts[5].trim().to_uppercase(),
            sql_redo: parts[6].trim().to_string(),
            sql_undo: parts[7].trim().to_string(),
        })
    }
}

fn logminer_start_scn(cursor: &LogMinerCursor) -> anyhow::Result<u64> {
    if cursor.has_row_position() {
        return Ok(cursor.scn);
    }
    cursor
        .scn
        .checked_add(1)
        .context("oracle logminer start_scn overflow")
}

fn build_cursor_predicate(cursor: &LogMinerCursor) -> String {
    if !cursor.has_row_position() {
        return format!("scn > {}", cursor.scn);
    }

    let rs_id = escape_sql_string(&cursor.rs_id);
    format!(
        "(scn > {scn} OR (scn = {scn} AND (rs_id > '{rs_id}' OR (rs_id = '{rs_id}' AND ssn > {ssn}))))",
        scn = cursor.scn,
        rs_id = rs_id,
        ssn = cursor.ssn
    )
}

fn build_seg_owner_table_predicate(captured: &[(String, String)]) -> anyhow::Result<String> {
    if captured.is_empty() {
        bail!("oracle logminer captured tables empty");
    }

    let mut out = Vec::with_capacity(captured.len());
    for (schema, tb) in captured {
        out.push(format!(
            "(SEG_OWNER = '{}' AND TABLE_NAME = '{}')",
            escape_sql_string(&schema.to_uppercase()),
            escape_sql_string(&tb.to_uppercase())
        ));
    }
    Ok(out.join(" OR "))
}

fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}
