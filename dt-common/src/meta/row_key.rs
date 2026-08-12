use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use anyhow::Context;

use super::{rdb_tb_meta::RdbTbMeta, row_data::RowData, row_type::RowType};

/// Values shorter than this are kept verbatim in a [`RowKey`]; longer ones are
/// reduced to a digest so that a key never carries a full copy of a large value.
/// Tables without a primary or unique key take *every* column as an id col
/// (see `RdbMetaManager::parse_rdb_cols`), so a key can otherwise be as large as
/// the row itself.
const INLINE_MAX_LEN: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RowKeyPart {
    Null,
    Inline(Box<str>),
    Digest { len: usize, hash: u64 },
}

impl RowKeyPart {
    fn new(col_value: Option<String>) -> Self {
        match col_value {
            None => Self::Null,
            Some(value) if value.len() <= INLINE_MAX_LEN => Self::Inline(value.into_boxed_str()),
            Some(value) => {
                let mut hasher = DefaultHasher::new();
                value.hash(&mut hasher);
                Self::Digest {
                    len: value.len(),
                    hash: hasher.finish(),
                }
            }
        }
    }
}

/// The identity of a row, built from the values of its id cols.
///
/// Unlike [`RowData::get_hash_code`], which returns 0 as soon as any id col is
/// NULL, a NULL takes part in equality here: two rows are the same only if every
/// id col value matches, NULL included. So a batch of rows that all carry a NULL
/// id col no longer collapses onto one map entry.
///
/// The per-col value is its `to_option_string()` form, i.e. exactly what
/// `ColValue::hash_code` hashes, so the equivalence between a source value and a
/// destination value is unchanged - including its deliberate cross-type reach,
/// where `Long(1)` and `String("1")` are the same key so heterogeneous source and
/// destination columns still line up.
///
/// Two rows whose id col values are all equal share a key, as they must; the
/// caller decides what to do with such duplicates, which only rdb tables lacking
/// a primary and unique key can hold.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RowKey {
    col_values: Vec<RowKeyPart>,
}

impl RowKey {
    pub fn from_row_data(row_data: &RowData, tb_meta: &RdbTbMeta) -> anyhow::Result<Self> {
        let col_values = match row_data.row_type {
            RowType::Insert => row_data
                .after
                .as_ref()
                .context("row_data after is missing")?,
            _ => row_data
                .before
                .as_ref()
                .context("row_data before is missing")?,
        };

        let mut key_values = Vec::with_capacity(tb_meta.id_cols.len());
        for col in tb_meta.id_cols.iter() {
            let col_value = col_values
                .get(col)
                .with_context(|| format!("missing id col value: {}", col))?;
            key_values.push(RowKeyPart::new(col_value.to_option_string()));
        }
        Ok(Self {
            col_values: key_values,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::meta::col_value::ColValue;

    fn tb_meta(id_cols: &[&str]) -> RdbTbMeta {
        RdbTbMeta {
            schema: "db1".into(),
            tb: "tb1".into(),
            id_cols: id_cols.iter().map(|col| col.to_string()).collect(),
            ..Default::default()
        }
    }

    fn col_values(values: &[(&str, ColValue)]) -> HashMap<String, ColValue> {
        values
            .iter()
            .map(|(col, value)| (col.to_string(), value.clone()))
            .collect()
    }

    fn insert_row(values: &[(&str, ColValue)]) -> RowData {
        RowData::new(
            "db1".into(),
            "tb1".into(),
            RowType::Insert,
            None,
            Some(col_values(values)),
        )
    }

    fn delete_row(values: &[(&str, ColValue)]) -> RowData {
        RowData::new(
            "db1".into(),
            "tb1".into(),
            RowType::Delete,
            Some(col_values(values)),
            None,
        )
    }

    fn key(row_data: &RowData, tb_meta: &RdbTbMeta) -> RowKey {
        RowKey::from_row_data(row_data, tb_meta).unwrap()
    }

    #[test]
    fn same_id_col_values_give_the_same_key() {
        let tb_meta = tb_meta(&["id", "value"]);
        let a = insert_row(&[("id", ColValue::Long(1)), ("value", ColValue::Long(2))]);
        let b = insert_row(&[
            ("id", ColValue::Long(1)),
            ("value", ColValue::Long(2)),
            ("other", ColValue::String("ignored".into())),
        ]);
        assert_eq!(key(&a, &tb_meta), key(&b, &tb_meta));
    }

    #[test]
    fn different_id_col_values_give_different_keys() {
        let tb_meta = tb_meta(&["id"]);
        let a = insert_row(&[("id", ColValue::Long(1))]);
        let b = insert_row(&[("id", ColValue::Long(2))]);
        assert_ne!(key(&a, &tb_meta), key(&b, &tb_meta));
    }

    #[test]
    fn null_id_col_values_are_compared_rather_than_collapsed() {
        let tb_meta = tb_meta(&["id", "value"]);
        // rows that only differ in a non-NULL id col must stay distinct even
        // though both carry a NULL - get_hash_code collapsed both onto 0
        let a = insert_row(&[("id", ColValue::Long(1)), ("value", ColValue::None)]);
        let b = insert_row(&[("id", ColValue::Long(2)), ("value", ColValue::None)]);
        assert_ne!(key(&a, &tb_meta), key(&b, &tb_meta));

        let a2 = insert_row(&[("id", ColValue::Long(1)), ("value", ColValue::None)]);
        assert_eq!(key(&a, &tb_meta), key(&a2, &tb_meta));
    }

    #[test]
    fn a_null_id_col_differs_from_a_non_null_one() {
        let tb_meta = tb_meta(&["id"]);
        let null_row = insert_row(&[("id", ColValue::None)]);
        let empty_string_row = insert_row(&[("id", ColValue::String(String::new()))]);
        assert_ne!(key(&null_row, &tb_meta), key(&empty_string_row, &tb_meta));
    }

    #[test]
    fn null_position_matters() {
        let tb_meta = tb_meta(&["id", "value"]);
        let a = insert_row(&[("id", ColValue::None), ("value", ColValue::Long(1))]);
        let b = insert_row(&[("id", ColValue::Long(1)), ("value", ColValue::None)]);
        assert_ne!(key(&a, &tb_meta), key(&b, &tb_meta));
    }

    #[test]
    fn a_batch_of_null_bearing_rows_keeps_one_map_entry_per_row() {
        let tb_meta = tb_meta(&["id", "value"]);
        let rows = vec![
            insert_row(&[("id", ColValue::Long(1)), ("value", ColValue::None)]),
            insert_row(&[("id", ColValue::Long(2)), ("value", ColValue::None)]),
            insert_row(&[("id", ColValue::None), ("value", ColValue::None)]),
        ];
        let map: HashMap<RowKey, &RowData> =
            rows.iter().map(|row| (key(row, &tb_meta), row)).collect();
        assert_eq!(map.len(), rows.len());
    }

    #[test]
    fn delete_and_update_rows_are_keyed_on_before() {
        let tb_meta = tb_meta(&["id"]);
        let deleted = delete_row(&[("id", ColValue::Long(1))]);
        let inserted = insert_row(&[("id", ColValue::Long(1))]);
        assert_eq!(key(&deleted, &tb_meta), key(&inserted, &tb_meta));
    }

    #[test]
    fn values_that_stringify_alike_share_a_key_as_before() {
        // heterogeneous src / dst cols rely on this: it is what
        // ColValue::hash_code already did by hashing to_option_string()
        let tb_meta = tb_meta(&["id"]);
        let long_row = insert_row(&[("id", ColValue::Long(1))]);
        let string_row = insert_row(&[("id", ColValue::String("1".into()))]);
        assert_eq!(key(&long_row, &tb_meta), key(&string_row, &tb_meta));
    }

    #[test]
    fn oversized_values_are_digested_yet_still_compare() {
        let tb_meta = tb_meta(&["id"]);
        let long_value = |fill: char| ColValue::String(fill.to_string().repeat(INLINE_MAX_LEN + 1));
        let a = insert_row(&[("id", long_value('a'))]);
        let a2 = insert_row(&[("id", long_value('a'))]);
        let b = insert_row(&[("id", long_value('b'))]);
        assert_eq!(key(&a, &tb_meta), key(&a2, &tb_meta));
        assert_ne!(key(&a, &tb_meta), key(&b, &tb_meta));
        // the value itself is not retained in the key
        assert!(matches!(
            key(&a, &tb_meta).col_values[0],
            RowKeyPart::Digest { .. }
        ));
    }

    #[test]
    fn a_missing_id_col_value_is_an_error() {
        let tb_meta = tb_meta(&["id"]);
        let row = insert_row(&[("other", ColValue::Long(1))]);
        let err = RowKey::from_row_data(&row, &tb_meta).unwrap_err();
        assert!(err.to_string().contains("missing id col value: id"));
    }

    #[test]
    fn a_missing_col_values_map_is_an_error() {
        let tb_meta = tb_meta(&["id"]);
        let row = RowData::new("db1".into(), "tb1".into(), RowType::Insert, None, None);
        let err = RowKey::from_row_data(&row, &tb_meta).unwrap_err();
        assert!(err.to_string().contains("after is missing"));
    }
}
