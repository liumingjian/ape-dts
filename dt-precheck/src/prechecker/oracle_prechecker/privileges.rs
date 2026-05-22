use std::collections::HashSet;

use super::OraclePrechecker;

impl OraclePrechecker {
    pub(super) async fn fetch_user_sys_privs(
        &self,
        required: &[&str],
    ) -> anyhow::Result<HashSet<String>> {
        if required.is_empty() {
            return Ok(HashSet::new());
        }

        let in_list = required
            .iter()
            .map(|p| format!("'{}'", p.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT privilege FROM user_sys_privs WHERE privilege IN ({})",
            in_list
        );
        let lines = self.fetcher.client()?.query_lines(&sql).await?;

        let mut out = HashSet::new();
        for line in lines {
            let s = line.trim().to_uppercase();
            if !s.is_empty() && s != "<NULL>" {
                out.insert(s);
            }
        }
        Ok(out)
    }
}
