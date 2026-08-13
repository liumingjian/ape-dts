use anyhow::{bail, Context};
use mongodb::bson::{doc, Bson, Document};

use dt_common::meta::mongo::mongo_constant::MongoConstants;

/// The update to apply on the target, parsed from the `o` field of an oplog `u` entry.
#[derive(Debug, Clone, PartialEq)]
pub enum MongoUpdate {
    /// operator style update, e.g. `{$set: {..}, $unset: {..}}`
    Diff(Document),
    /// full replacement document, produced by `replaceOne` style updates
    Replace(Document),
}

/// Parses oplog update entries into an update the mongo sinker can apply.
///
/// MongoDB 4.4 introduced the `$v: 2` oplog format, where `o` carries a *diff tree*
/// instead of a flat `$set` / `$unset`. The tree is recursive:
///
/// - document diff: `i` / `u` (inserted / updated sub fields), `d` (deleted sub fields),
///   `s<field>` (nested diff of `<field>`)
/// - array diff: `a: true` marker, `l` (new length, i.e. the array was truncated),
///   `u<index>` (new element at `<index>`), `s<index>` (nested diff of element `<index>`)
///
/// Everything is flattened into dotted paths, so `{sa: {su: {u: {b: 1}}}}` becomes
/// `{$set: {"a.u.b": 1}}`.
pub struct MongoDiff {}

#[derive(Default)]
struct Acc {
    set: Document,
    unset: Document,
    push: Document,
}

impl MongoDiff {
    /// `o` is the `o` field of an oplog entry whose `op` is `u`.
    pub fn parse_update(o: &Document) -> anyhow::Result<MongoUpdate> {
        // MongoDB >= 4.4: {"$v": 2, "diff": {..}}.
        // `$v` must say 2: a replacement doc is free to carry a user field named `diff`,
        // and expanding that as a diff tree would silently write the wrong fields
        if Self::is_v2(o) {
            let diff = o
                .get("diff")
                .with_context(|| format!("`$v: 2` update oplog has no `diff`: {:?}", o))?;
            let diff = Self::as_document(diff, "diff")?;
            return Ok(MongoUpdate::Diff(Self::expand_v2_diff(diff)?));
        }

        // MongoDB <= 4.2 (and $v: 1): {"$set": {..}, "$unset": {..}}
        let has_operator = o.keys().any(|k| k.starts_with('$') && k != "$v");
        if has_operator {
            let mut update = Document::new();
            for (key, value) in o.iter() {
                match key.as_str() {
                    "$v" => continue,
                    MongoConstants::SET | MongoConstants::UNSET => {
                        let sub = Self::as_document(value, key)?;
                        if !sub.is_empty() {
                            update.insert(key, sub.clone());
                        }
                    }
                    _ => bail!("unsupported update operator in oplog `o`: `{}`", key),
                }
            }
            if update.is_empty() {
                bail!("update oplog carries an empty $set/$unset");
            }
            return Ok(MongoUpdate::Diff(update));
        }

        // no operator at all: the whole doc is the new version of the document.
        // a leftover `$v` (or any other `$` key) means we failed to recognise the entry,
        // replacing the target doc with it would wipe the real fields
        if let Some(key) = o.keys().find(|k| k.starts_with('$')) {
            bail!(
                "update oplog `o` carries `{}` but no update we can apply: {:?}",
                key,
                o
            );
        }
        Ok(MongoUpdate::Replace(o.clone()))
    }

    fn is_v2(o: &Document) -> bool {
        matches!(o.get("$v"), Some(Bson::Int32(2)) | Some(Bson::Int64(2)))
    }

    /// Flattens a `$v: 2` diff tree into `{$set: {..}, $unset: {..}, $push: {..}}`
    /// with dotted paths.
    pub fn expand_v2_diff(diff: &Document) -> anyhow::Result<Document> {
        let mut acc = Acc::default();
        Self::walk(diff, "", &mut acc)?;

        if acc.set.is_empty() && acc.unset.is_empty() && acc.push.is_empty() {
            bail!("$v: 2 diff expands to neither $set nor $unset");
        }

        let mut update = Document::new();
        if !acc.set.is_empty() {
            update.insert(MongoConstants::SET, acc.set);
        }
        if !acc.unset.is_empty() {
            update.insert(MongoConstants::UNSET, acc.unset);
        }
        if !acc.push.is_empty() {
            update.insert(MongoConstants::PUSH, acc.push);
        }
        Ok(update)
    }

    fn walk(diff: &Document, prefix: &str, acc: &mut Acc) -> anyhow::Result<()> {
        if Self::is_array_diff(diff) {
            Self::walk_array(diff, prefix, acc)
        } else {
            Self::walk_doc(diff, prefix, acc)
        }
    }

    fn is_array_diff(diff: &Document) -> bool {
        matches!(diff.get("a"), Some(Bson::Boolean(true)))
    }

    fn walk_doc(diff: &Document, prefix: &str, acc: &mut Acc) -> anyhow::Result<()> {
        for (key, value) in diff.iter() {
            match key.as_str() {
                // inserted / updated sub fields
                "i" | "u" => {
                    for (field, new_value) in Self::as_document(value, key)?.iter() {
                        acc.set
                            .insert(Self::join(prefix, field)?, new_value.clone());
                    }
                }

                // deleted sub fields
                "d" => {
                    for (field, _) in Self::as_document(value, key)?.iter() {
                        acc.unset.insert(Self::join(prefix, field)?, "");
                    }
                }

                // nested diff of a sub document / array
                _ if key.len() > 1 && key.starts_with('s') => {
                    let field = &key[1..];
                    let nested_prefix = format!("{}.", Self::join(prefix, field)?);
                    Self::walk(Self::as_document(value, key)?, &nested_prefix, acc)?;
                }

                _ => bail!(
                    "unsupported key `{}` in $v: 2 document diff at path `{}`",
                    key,
                    Self::path_of(prefix)
                ),
            }
        }
        Ok(())
    }

    fn walk_array(diff: &Document, prefix: &str, acc: &mut Acc) -> anyhow::Result<()> {
        // a pure truncation ({a: true, l: n}) replays as $push with an empty $each and a $slice.
        // it only works on its own: $push and $set would both claim the array path, and mongo
        // rejects an update whose operators conflict on one path
        let is_pure_truncation = diff.len() == 2 && diff.contains_key("l");

        for (key, value) in diff.iter() {
            match key.as_str() {
                // the array marker itself
                "a" => continue,

                // the array was truncated to `l` elements
                "l" => {
                    let new_len = value.as_i32().map(|v| v as i64).or_else(|| value.as_i64());
                    match (is_pure_truncation, new_len) {
                        (true, Some(new_len)) if new_len >= 0 => {
                            acc.push.insert(
                                Self::path_of(prefix).to_string(),
                                doc! {"$each": [], "$slice": new_len},
                            );
                        }
                        _ => bail!(
                            "array at path `{}` was resized to {:?} together with element changes, which can not be replayed without the whole array",
                            Self::path_of(prefix),
                            value
                        ),
                    }
                }

                // new element at index
                _ if key.len() > 1 && key.starts_with('u') => {
                    let index = Self::as_index(&key[1..], prefix)?;
                    acc.set.insert(Self::join(prefix, &index)?, value.clone());
                }

                // nested diff of the element at index
                _ if key.len() > 1 && key.starts_with('s') => {
                    let index = Self::as_index(&key[1..], prefix)?;
                    let nested_prefix = format!("{}.", Self::join(prefix, &index)?);
                    Self::walk(Self::as_document(value, key)?, &nested_prefix, acc)?;
                }

                _ => bail!(
                    "unsupported key `{}` in $v: 2 array diff at path `{}`",
                    key,
                    Self::path_of(prefix)
                ),
            }
        }
        Ok(())
    }

    fn as_document<'a>(value: &'a Bson, key: &str) -> anyhow::Result<&'a Document> {
        match value.as_document() {
            Some(doc) => Ok(doc),
            None => bail!("`{}` in $v: 2 diff is not a document: {:?}", key, value),
        }
    }

    fn as_index(raw: &str, prefix: &str) -> anyhow::Result<String> {
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            bail!(
                "`{}` is not an array index in $v: 2 array diff at path `{}`",
                raw,
                Self::path_of(prefix)
            );
        }
        Ok(raw.to_string())
    }

    /// A field name carrying `.` or a leading `$` would turn into a dotted path pointing
    /// at a *different* field, silently corrupting the target, so refuse it instead.
    fn join(prefix: &str, field: &str) -> anyhow::Result<String> {
        if field.is_empty() || field.contains('.') || field.starts_with('$') {
            bail!(
                "field name `{}` at path `{}` can not be expressed as a dotted update path",
                field,
                Self::path_of(prefix)
            );
        }
        Ok(format!("{}{}", prefix, field))
    }

    fn path_of(prefix: &str) -> &str {
        if prefix.is_empty() {
            "<root>"
        } else {
            prefix.trim_end_matches('.')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    fn expand(diff: Document) -> Document {
        MongoDiff::expand_v2_diff(&diff).unwrap()
    }

    #[test]
    fn test_flat_diff_keeps_all_of_i_u_d() {
        // db.tb.updateOne({_id: 1}, {$set: {a: 1, b: 2}, $unset: {c: ""}})
        let diff = doc! {
            "i": {"a": 1},
            "u": {"b": 2},
            "d": {"c": false},
        };
        assert_eq!(
            expand(diff),
            doc! {"$set": {"a": 1, "b": 2}, "$unset": {"c": ""}}
        );
    }

    #[test]
    fn test_nested_document_diff() {
        // db.tb.updateOne({_id: 1}, {$set: {"a.b.c": 1}})
        let diff = doc! {
            "sa": {"sb": {"u": {"c": 1}}},
        };
        assert_eq!(expand(diff), doc! {"$set": {"a.b.c": 1}});
    }

    #[test]
    fn test_nested_document_unset() {
        let diff = doc! {
            "sa": {"d": {"b": false}},
        };
        assert_eq!(expand(diff), doc! {"$unset": {"a.b": ""}});
    }

    #[test]
    fn test_array_element_update() {
        // db.tb.updateOne({_id: 1}, {$set: {"arr.1": 9}})
        let diff = doc! {
            "sarr": {"a": true, "u1": 9},
        };
        assert_eq!(expand(diff), doc! {"$set": {"arr.1": 9}});
    }

    #[test]
    fn test_array_element_nested_diff() {
        // db.tb.updateOne({_id: 1}, {$set: {"arr.0.x": 1}, $unset: {"arr.0.y": ""}})
        let diff = doc! {
            "sarr": {"a": true, "s0": {"u": {"x": 1}, "d": {"y": false}}},
        };
        assert_eq!(
            expand(diff),
            doc! {"$set": {"arr.0.x": 1}, "$unset": {"arr.0.y": ""}}
        );
    }

    #[test]
    fn test_deeply_nested_mixed_diff() {
        let diff = doc! {
            "u": {"top": 1},
            "sa": {
                "i": {"new": true},
                "sarr": {
                    "a": true,
                    "u2": {"replaced": 1},
                    "s0": {"a": true, "u1": "deep"},
                },
            },
        };
        assert_eq!(
            expand(diff),
            doc! {"$set": {
                "top": 1,
                "a.new": true,
                "a.arr.2": {"replaced": 1},
                "a.arr.0.1": "deep",
            }}
        );
    }

    #[test]
    fn test_pure_array_truncation_replays_as_push_slice() {
        // $pop / $pull of trailing elements: the array only shrank
        let diff = doc! {"sarr": {"a": true, "l": 2}};
        assert_eq!(
            expand(diff),
            doc! {"$push": {"arr": {"$each": [], "$slice": 2i64}}}
        );
    }

    #[test]
    fn test_array_resize_with_element_changes_is_rejected() {
        // $push and $set would both claim `arr`, and the old content is unknown anyway
        let diff = doc! {"sarr": {"a": true, "l": 2, "u0": 9}};
        let err = MongoDiff::expand_v2_diff(&diff).unwrap_err().to_string();
        assert!(err.contains("arr"), "{}", err);
        assert!(err.contains("resized"), "{}", err);
    }

    #[test]
    fn test_replacement_doc_with_a_diff_field_is_not_a_v2_diff() {
        // a replaceOne whose doc happens to carry a `diff` field must not be expanded
        let o = doc! {"_id": 1, "diff": {"u": {"x": 1}}};
        assert_eq!(
            MongoDiff::parse_update(&o).unwrap(),
            MongoUpdate::Replace(o.clone())
        );
    }

    #[test]
    fn test_unknown_key_is_rejected() {
        let diff = doc! {"sa": {"z": {"b": 1}}};
        let err = MongoDiff::expand_v2_diff(&diff).unwrap_err().to_string();
        assert!(err.contains("unsupported key `z`"), "{}", err);
        assert!(err.contains("`a`"), "{}", err);
    }

    #[test]
    fn test_dotted_field_name_is_rejected() {
        let diff = doc! {"u": {"a.b": 1}};
        let err = MongoDiff::expand_v2_diff(&diff).unwrap_err().to_string();
        assert!(err.contains("dotted update path"), "{}", err);
    }

    #[test]
    fn test_non_numeric_array_index_is_rejected() {
        let diff = doc! {"sarr": {"a": true, "ux": 1}};
        let err = MongoDiff::expand_v2_diff(&diff).unwrap_err().to_string();
        assert!(err.contains("not an array index"), "{}", err);
    }

    #[test]
    fn test_empty_diff_is_rejected() {
        let err = MongoDiff::expand_v2_diff(&doc! {}).unwrap_err().to_string();
        assert!(err.contains("neither $set nor $unset"), "{}", err);
    }

    #[test]
    fn test_parse_update_v2() {
        let o = doc! {"$v": 2, "diff": {"sa": {"u": {"b": 1}}}};
        assert_eq!(
            MongoDiff::parse_update(&o).unwrap(),
            MongoUpdate::Diff(doc! {"$set": {"a.b": 1}})
        );
    }

    #[test]
    fn test_parse_update_v1_keeps_both_set_and_unset() {
        let o = doc! {"$v": 1, "$set": {"a": 1}, "$unset": {"b": true}};
        assert_eq!(
            MongoDiff::parse_update(&o).unwrap(),
            MongoUpdate::Diff(doc! {"$set": {"a": 1}, "$unset": {"b": true}})
        );
    }

    #[test]
    fn test_parse_update_replacement_doc() {
        let o = doc! {"_id": 1, "a": 1};
        assert_eq!(
            MongoDiff::parse_update(&o).unwrap(),
            MongoUpdate::Replace(doc! {"_id": 1, "a": 1})
        );
    }

    #[test]
    fn test_parse_update_version_only_is_rejected() {
        // {"$v": 1} alone is not a replacement doc, replaying it would wipe the target doc
        let err = MongoDiff::parse_update(&doc! {"$v": 1})
            .unwrap_err()
            .to_string();
        assert!(err.contains("no update we can apply"), "{}", err);
    }

    #[test]
    fn test_parse_update_unsupported_operator() {
        let o = doc! {"$inc": {"a": 1}};
        let err = MongoDiff::parse_update(&o).unwrap_err().to_string();
        assert!(err.contains("unsupported update operator"), "{}", err);
    }
}
