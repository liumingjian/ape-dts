use anyhow::{bail, Context};

use crate::oracle::OracleSqlPlusClient;

#[derive(Debug, Clone)]
pub(crate) struct LogMinerRow {
    pub(crate) scn: u64,
    pub(crate) operation: String,
    pub(crate) schema: String,
    pub(crate) tb: String,
    pub(crate) sql_redo: String,
    pub(crate) sql_undo: String,
}

pub(crate) async fn current_scn(client: &OracleSqlPlusClient) -> anyhow::Result<u64> {
    let lines = client.query_lines("SELECT CURRENT_SCN FROM V$DATABASE").await?;
    let first = lines.first().context("oracle current_scn query returned no rows")?;
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

pub(crate) async fn start_logminer_session(
    client: &OracleSqlPlusClient,
    redo_logs: &[String],
    start_scn: u64,
    end_scn: u64,
) -> anyhow::Result<()> {
    if start_scn == 0 || end_scn == 0 || end_scn < start_scn {
        bail!(
            "invalid logminer scn range: start_scn={}, end_scn={}",
            start_scn,
            end_scn
        );
    }
    if redo_logs.is_empty() {
        bail!("oracle logminer redo_logs empty");
    }

    let mut script = String::from("BEGIN\n");
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

    client.exec(&script).await
}

pub(crate) async fn end_logminer_session(client: &OracleSqlPlusClient) -> anyhow::Result<()> {
    client.exec("BEGIN DBMS_LOGMNR.END_LOGMNR; END;\n/").await
}

pub(crate) async fn fetch_logmnr_rows(
    client: &OracleSqlPlusClient,
    captured: &[(String, String)],
    limit: usize,
) -> anyhow::Result<Vec<LogMinerRow>> {
    let pairs = build_seg_owner_table_predicate(captured)?;
    let sql = format!(
        "SELECT scn, operation, seg_owner, table_name, sql_redo, sql_undo FROM (SELECT scn, operation, seg_owner, table_name, sql_redo, sql_undo FROM V$LOGMNR_CONTENTS WHERE operation IN ('INSERT','UPDATE','DELETE') AND ({pairs}) ORDER BY scn ASC) WHERE ROWNUM <= {limit}",
        pairs = pairs,
        limit = limit
    );

    let lines = client.query_lines(&sql).await?;
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        out.push(LogMinerRow::from_line(&line)?);
    }
    Ok(out)
}

impl LogMinerRow {
    fn from_line(line: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 6 {
            bail!("oracle logminer row parse failed (expected 6 cols), line={}", line);
        }

        Ok(Self {
            scn: parts[0]
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid scn in logminer line: {}", line))?,
            operation: parts[1].trim().to_uppercase(),
            schema: parts[2].trim().to_uppercase(),
            tb: parts[3].trim().to_uppercase(),
            sql_redo: parts[4].trim().to_string(),
            sql_undo: parts[5].trim().to_string(),
        })
    }
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

