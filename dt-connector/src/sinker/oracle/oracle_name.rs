use anyhow::bail;
use sha2::{Digest, Sha256};

const ORACLE_CONSTRAINT_HASH_LEN: usize = 8;
const ORACLE_IDENT_MAX_LEN: usize = 30;
const PG_DEFAULT_SCHEMA: &str = "PUBLIC";

pub(crate) fn oracle_ident(raw: &str) -> anyhow::Result<String> {
    let ident = raw.trim().trim_matches('"').to_uppercase();
    if ident.is_empty() {
        bail!("oracle ident is empty");
    }
    if ident.len() > ORACLE_IDENT_MAX_LEN {
        bail!(
            "oracle ident too long (max={}): {}",
            ORACLE_IDENT_MAX_LEN,
            ident
        );
    }
    Ok(ident)
}

pub(crate) fn oracle_constraint_ident(raw: &str) -> anyhow::Result<String> {
    let ident = raw.trim().trim_matches('"').to_uppercase();
    if ident.is_empty() {
        bail!("oracle constraint ident is empty");
    }
    if ident.len() <= ORACLE_IDENT_MAX_LEN {
        return Ok(ident);
    }

    let digest = Sha256::digest(ident.as_bytes());
    let suffix = hex::encode(&digest[..4]).to_uppercase();
    let prefix_len = ORACLE_IDENT_MAX_LEN - ORACLE_CONSTRAINT_HASH_LEN - 1;
    Ok(format!("{}_{}", &ident[..prefix_len], suffix))
}

pub(crate) fn oracle_owner_expr(schema: &str) -> anyhow::Result<String> {
    let schema = oracle_ident(schema)?;
    if schema == PG_DEFAULT_SCHEMA {
        return Ok("USER".to_string());
    }
    Ok(format!("'{}'", schema))
}

pub(crate) fn oracle_table_ref(schema: &str, table: &str) -> anyhow::Result<String> {
    let schema = oracle_ident(schema)?;
    let table = oracle_ident(table)?;
    if schema == PG_DEFAULT_SCHEMA {
        return Ok(table);
    }
    Ok(format!("{}.{}", schema, table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_constraint_ident_shortens_long_names_deterministically() {
        let name = oracle_constraint_ident("t_gaussdb_oracle_to_oracle_pkey").unwrap();

        assert_eq!(name.len(), ORACLE_IDENT_MAX_LEN);
        assert!(name.starts_with("T_GAUSSDB_ORACLE_TO_"));
        assert_eq!(
            name,
            oracle_constraint_ident("t_gaussdb_oracle_to_oracle_pkey").unwrap()
        );
    }

    #[test]
    fn oracle_ident_still_rejects_long_table_names() {
        let err = oracle_ident("t_gaussdb_oracle_to_oracle_too_long").unwrap_err();

        assert!(err.to_string().contains("oracle ident too long"));
    }

    #[test]
    fn oracle_table_ref_uses_current_oracle_schema_for_pg_public() {
        assert_eq!(
            oracle_table_ref("public", "t_gaussdb_oracle_to_oracle").unwrap(),
            "T_GAUSSDB_ORACLE_TO_ORACLE"
        );
        assert_eq!(oracle_owner_expr("public").unwrap(), "USER");
    }

    #[test]
    fn oracle_table_ref_preserves_explicit_oracle_schema() {
        assert_eq!(
            oracle_table_ref("ape_dts", "gdbora").unwrap(),
            "APE_DTS.GDBORA"
        );
        assert_eq!(oracle_owner_expr("ape_dts").unwrap(), "'APE_DTS'");
    }
}
