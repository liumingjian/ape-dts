use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    extractor::{
        base_extractor::BaseExtractor,
        mongo::mongo_diff::{MongoDiff, MongoUpdate},
        resumer::recovery::Recovery,
    },
    Extractor,
};
use anyhow::{bail, Context};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use dt_common::{
    config::config_enums::DbType,
    log_error, log_info, log_warn,
    meta::{
        col_value::ColValue,
        dt_data::DtData,
        mongo::{
            mongo_cdc_source::MongoCdcSource, mongo_constant::MongoConstants,
            mongo_diff_policy::MongoUnsupportedDiffPolicy,
        },
        position::Position,
        row_data::RowData,
        row_type::RowType,
        syncer::Syncer,
    },
    rdb_filter::RdbFilter,
    system_dbs::SystemDb,
    utils::time_util::TimeUtil,
};
use mongodb::{
    bson::{doc, Bson, Document, Timestamp},
    change_stream::event::{OperationType, ResumeToken, UpdateDescription},
    options::{ChangeStreamOptions, FullDocumentBeforeChangeType, FullDocumentType, UpdateOptions},
    Client,
};

pub struct MongoCdcExtractor {
    pub base_extractor: BaseExtractor,
    pub filter: RdbFilter,
    pub resume_token: String,
    pub start_timestamp: u32,
    pub source: MongoCdcSource,
    pub mongo_client: Client,
    pub app_name: String,
    pub heartbeat_interval_secs: u64,
    pub heartbeat_tb: String,
    pub syncer: Arc<Mutex<Syncer>>,
    pub recovery: Option<Arc<dyn Recovery + Send + Sync>>,
    pub on_unsupported_diff: MongoUnsupportedDiffPolicy,
}

#[async_trait]
impl Extractor for MongoCdcExtractor {
    async fn extract(&mut self) -> anyhow::Result<()> {
        if let Some(recovery) = &self.recovery {
            if let Some(position) = recovery.get_cdc_resume_position().await {
                match &position {
                    Position::MongoCdc {
                        resume_token,
                        operation_time,
                        ..
                    } => {
                        self.resume_token = resume_token.to_owned();
                        self.start_timestamp = operation_time.to_owned();
                        log_info!(
                            "cdc recovery from resume_token:[{}], operation_time:[{}]",
                            resume_token,
                            operation_time
                        );
                        self.base_extractor
                            .push_dt_data(DtData::Heartbeat {}, position)
                            .await?;
                    }
                    _ => {
                        log_warn!("position:{} is not a valid mongo cdc position", position);
                    }
                }
            }
        }

        log_info!(
            "MongoCdcExtractor starts, resume_token: {}, start_timestamp: {}, source: {:?} ",
            self.resume_token,
            self.start_timestamp,
            self.source,
        );

        // start heartbeat
        self.start_heartbeat(self.base_extractor.cancel_token.clone())?;

        match self.source {
            MongoCdcSource::OpLog => self.extract_oplog().await?,
            MongoCdcSource::ChangeStream => self.extract_change_stream().await?,
        }
        self.base_extractor.wait_task_finish().await
    }

    async fn close(&mut self) -> anyhow::Result<()> {
        self.mongo_client.clone().shutdown().await;
        Ok(())
    }
}

impl MongoCdcExtractor {
    async fn extract_oplog(&mut self) -> anyhow::Result<()> {
        let start_timestamp = self.parse_start_timestamp();
        let filter = doc! {
            "ts": { "$gte": start_timestamp }
        };
        let options = mongodb::options::FindOptions::builder()
            .cursor_type(mongodb::options::CursorType::TailableAwait)
            .build();

        let oplog = self
            .mongo_client
            .database("local")
            .collection::<Document>("oplog.rs");
        let mut cursor = oplog.find(filter, options).await?;

        while cursor.advance().await? {
            let doc: Document = cursor.deserialize_current()?;
            // https://github.com/mongodb/mongo/blob/master/src/mongo/db/repl/oplog.cpp
            // op:
            //     "i" insert
            //     "u" update
            //     "d" delete
            //     "c" db cmd
            //     "n" no op
            //     "xi" insert global index key
            //     "xd" delete global index key

            let op = Self::get_op(&doc);
            let mut row_type = RowType::Insert;
            let mut before = HashMap::new();
            let mut after = HashMap::new();
            let o = doc.get("o");
            let o2 = doc.get("o2");
            let ts = doc.get("ts");
            let ns = doc.get("ns");

            match op.as_str() {
                "i" => {
                    after.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(Self::require_doc(&o, "o")?.clone()),
                    );
                }
                "u" => {
                    row_type = RowType::Update;
                    // for update op log, doc.o contains only the diff instead of the full doc
                    // refer: https://www.mongodb.com/community/forums/t/oplog-update-entry-without-set-and-unset/171771
                    // https://www.mongodb.com/docs/manual/reference/operator/update/#update-operators-1
                    // in MongoDB 4.2 and earlier, o contains $set / $unset with the changed fields,
                    // from 4.4 on, o contains a `$v: 2` diff tree which needs expanding.
                    let o_doc = Self::require_doc(&o, "o")?;
                    let o2_doc = Self::require_doc(&o2, "o2")?;

                    match MongoDiff::parse_update(o_doc) {
                        Ok(MongoUpdate::Diff(diff_doc)) => {
                            after.insert(
                                MongoConstants::DIFF_DOC.to_string(),
                                ColValue::MongoDoc(diff_doc),
                            );
                        }

                        // replacement style update: o is the new version of the whole doc
                        Ok(MongoUpdate::Replace(new_doc)) => {
                            after.insert(
                                MongoConstants::DOC.to_string(),
                                ColValue::MongoDoc(new_doc),
                            );
                        }

                        Err(err) => {
                            self.on_unsupported_diff(err, &format!("o2: {:?}, o: {:?}", o2, o))?;
                            continue;
                        }
                    }

                    before.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(o2_doc.clone()),
                    );
                }
                "d" => {
                    row_type = RowType::Delete;
                    before.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(Self::require_doc(&o, "o")?.clone()),
                    );
                }
                // TODO, DDL
                "c" | "xi" | "xd" => {
                    // after version 7.0, the oplog generated by deleteMany is "c" instead of "d"
                    let data = Self::extract_oplog_delete_many(&doc)?;
                    for (row_data, position) in data {
                        self.push_row_to_buf(row_data, position).await?;
                    }
                    continue;
                }
                "n" => {
                    // TODO, heartbeat
                    // Document({"op": String("n"), "ns": String(""), "o": Document({"msg": String("periodic noop")}), "ts": Timestamp { time: 1693470874, increment: 1 }, "t": Int64(67), "v": Int64(2), "wall": DateTime(2023-08-31 8:34:34.19 +00:00:00)})
                    continue;
                }
                _ => {
                    continue;
                }
            }

            // get db & tb
            let (row_data, position) =
                Self::build_oplog_row_data(&ns, &ts, row_type, before, after)?;
            self.push_row_to_buf(row_data, position).await?;
        }
        Ok(())
    }

    fn get_op(doc: &Document) -> String {
        if doc.get("op").is_none() || doc.get("op").unwrap().as_str().is_none() {
            return String::new();
        }
        let op = doc.get("op").unwrap().as_str().unwrap();
        op.into()
    }

    fn extract_oplog_delete_many(doc: &Document) -> anyhow::Result<Vec<(RowData, Position)>> {
        // Some(Document({
        //     "applyOps": Array([Document({
        //         "op": String("d"),
        //         "ns": String("test_db_2.tb_1"),
        //         "ui": Binary {
        //             subtype: Uuid,
        //             bytes: [253, 133, 25, 188, 63, 140, 74, 157, 141, 86, 245, 125, 168, 32, 95, 231]
        //         },
        //         "o": Document({
        //             "_id": String("1")
        //         })
        //     }), Document({
        //         "op": String("d"),
        //         "ns": String("test_db_2.tb_1"),
        //         "ui": Binary {
        //             subtype: Uuid,
        //             bytes: [253, 133, 25, 188, 63, 140, 74, 157, 141, 86, 245, 125, 168, 32, 95, 231]
        //         },
        //         "o": Document({
        //             "_id": String("2")
        //         })
        //     })])
        // }))

        let mut data = vec![];
        let o = doc.get("o");
        let ts = doc.get("ts");

        if o.is_none() || o.unwrap().as_document().is_none() {
            return Ok(data);
        }

        let doc = o.unwrap().as_document().unwrap();
        if doc.get("applyOps").is_none() {
            return Ok(data);
        }

        let apply_ops = doc.get("applyOps").unwrap();
        if apply_ops.as_array().is_none() {
            return Ok(data);
        }

        for ops in apply_ops.as_array().unwrap() {
            if ops.as_document().is_none() {
                continue;
            }

            let item = ops.as_document().unwrap();
            let op = Self::get_op(item);
            let ns = item.get("ns");

            if op.as_str() != "d" {
                continue;
            }

            let o = item.get("o");
            let mut before = HashMap::new();
            before.insert(
                MongoConstants::DOC.to_string(),
                ColValue::MongoDoc(Self::require_doc(&o, "applyOps.o")?.clone()),
            );

            data.push(Self::build_oplog_row_data(
                &ns,
                &ts,
                RowType::Delete,
                before,
                HashMap::new(),
            )?);
        }
        Ok(data)
    }

    fn build_oplog_row_data(
        ns: &Option<&Bson>,
        ts: &Option<&Bson>,
        row_type: RowType,
        before: HashMap<String, ColValue>,
        after: HashMap<String, ColValue>,
    ) -> anyhow::Result<(RowData, Position)> {
        let ts = ts
            .and_then(|v| v.as_timestamp())
            .context("op_log entry has no valid `ts`")?;
        let ns = ns
            .and_then(|v| v.as_str())
            .context("op_log entry has no valid `ns`")?;

        // get db & tb
        let tokens: Vec<&str> = ns.split('.').collect();
        let db: String = tokens[0].into();
        let tb: String = ns[db.len() + 1..].into();
        let before = if before.is_empty() {
            None
        } else {
            Some(before)
        };
        let after = if after.is_empty() { None } else { Some(after) };

        // get ts for position
        let position = Position::MongoCdc {
            resume_token: String::new(),
            operation_time: ts.time,
            timestamp: Position::format_timestamp_millis(ts.time as i64 * 1000),
        };
        let row_data = RowData::new(db, tb, row_type, before, after);
        Ok((row_data, position))
    }

    async fn extract_change_stream(&mut self) -> anyhow::Result<()> {
        let (resume_token, start_timestamp) = if self.resume_token.is_empty() {
            (None, Some(self.parse_start_timestamp()))
        } else {
            let token: ResumeToken = serde_json::from_str(&self.resume_token)?;
            (Some(token), None)
        };

        // refer: https://www.mongodb.com/docs/manual/changeStreams/
        // Starting in MongoDB 6.0, you can use change stream events to output the version of
        // a document before and after changes (the document pre- and post-images)
        let stream_options = ChangeStreamOptions::builder()
            .start_at_operation_time(start_timestamp)
            .start_after(resume_token)
            .full_document(Some(FullDocumentType::UpdateLookup))
            .full_document_before_change(Some(FullDocumentBeforeChangeType::WhenAvailable))
            .build();

        let mut change_stream = self.mongo_client.watch(None, stream_options).await?;
        let cancel_token = self.base_extractor.cancel_token.clone();

        loop {
            // await the next event instead of polling with next_if_any, which used to spin
            // a whole cpu core on an idle stream
            let next = tokio::select! {
                biased;
                _ = cancel_token.cancelled() => return Ok(()),
                next = change_stream.next() => next,
            };

            let doc = match next {
                Some(result) => result?,
                // the stream ended, nothing more to extract
                None => return Ok(()),
            };

            let resume_token = doc.id;
            let position = if let Some(operation_time) = doc.cluster_time {
                Position::MongoCdc {
                    resume_token: json!(resume_token).to_string(),
                    operation_time: operation_time.time,
                    timestamp: Position::format_timestamp_millis(operation_time.time as i64 * 1000),
                }
            } else {
                Position::MongoCdc {
                    resume_token: json!(resume_token).to_string(),
                    operation_time: 0,
                    timestamp: String::new(),
                }
            };

            let (mut db, mut tb) = (String::new(), String::new());
            if let Some(ns) = doc.ns {
                db = ns.db.clone();
                if let Some(coll) = ns.coll {
                    tb = coll.clone();
                }
            }

            let mut row_type = RowType::Insert;
            let mut before = HashMap::new();
            let mut after = HashMap::new();

            match doc.operation_type {
                OperationType::Insert => {
                    let full_document = doc
                        .full_document
                        .context("change stream insert event has no fullDocument")?;
                    after.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(full_document),
                    );
                }

                OperationType::Delete => {
                    row_type = RowType::Delete;
                    let document_key = doc
                        .document_key
                        .context("change stream delete event has no documentKey")?;
                    before.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(document_key),
                    );
                }

                OperationType::Update | OperationType::Replace => {
                    row_type = RowType::Update;
                    let document_key = doc
                        .document_key
                        .context("change stream update event has no documentKey")?;

                    if let Some(document) = doc.full_document {
                        after.insert(MongoConstants::DOC.to_string(), ColValue::MongoDoc(document));
                    } else if doc.operation_type == OperationType::Replace {
                        // a replace event carries no updateDescription, and the post image is
                        // missing only when the doc was deleted right after: the delete event
                        // that follows brings the target back in sync
                        log_warn!(
                            "change stream replace event has no fullDocument, the doc is already gone, skipping it, document_key: {:?}",
                            document_key
                        );
                        continue;
                    } else {
                        // the post image is unavailable, typically because the doc was deleted
                        // before the update lookup ran, fall back to the update description,
                        // which already carries dotted paths. it must NOT upsert: the doc no
                        // longer exists on the source, recreating it from the changed fields
                        // alone would leave a ghost on the target
                        match Self::change_stream_diff_doc(&doc.update_description) {
                            Ok(diff_doc) => {
                                after.insert(
                                    MongoConstants::DIFF_DOC_NO_UPSERT.to_string(),
                                    ColValue::MongoDoc(diff_doc),
                                );
                            }
                            Err(err) => {
                                self.on_unsupported_diff(
                                    err,
                                    &format!("document_key: {:?}", document_key),
                                )?;
                                continue;
                            }
                        }
                    }

                    before.insert(
                        MongoConstants::DOC.to_string(),
                        ColValue::MongoDoc(document_key),
                    );
                }

                // TODO, heartbeat and DDL
                _ => {
                    continue;
                }
            }

            let row_data = RowData::new(db, tb, row_type, Some(before), Some(after));
            self.push_row_to_buf(row_data, position).await?;
        }
    }

    /// Builds a `{$set: .., $unset: ..}` update out of a change stream `updateDescription`,
    /// used when the post image of an update is not available.
    fn change_stream_diff_doc(
        update_description: &Option<UpdateDescription>,
    ) -> anyhow::Result<Document> {
        let update_description = match update_description {
            Some(update_description) => update_description,
            None => bail!("change stream update event has neither fullDocument nor updateDescription"),
        };

        if let Some(truncated_arrays) = &update_description.truncated_arrays {
            if !truncated_arrays.is_empty() {
                bail!(
                    "change stream update event truncated arrays {:?}, which can not be replayed without the post image",
                    truncated_arrays
                );
            }
        }

        let mut update = Document::new();
        if !update_description.updated_fields.is_empty() {
            update.insert(
                MongoConstants::SET,
                update_description.updated_fields.clone(),
            );
        }
        if !update_description.removed_fields.is_empty() {
            let mut unset = Document::new();
            for field in update_description.removed_fields.iter() {
                unset.insert(field, "");
            }
            update.insert(MongoConstants::UNSET, unset);
        }

        if update.is_empty() {
            bail!("change stream update event has an empty updateDescription");
        }
        Ok(update)
    }

    /// Applies the configured policy to an update we can not replay: fail the task by default,
    /// so the target does not silently diverge from the source.
    fn on_unsupported_diff(&self, err: anyhow::Error, context: &str) -> anyhow::Result<()> {
        match self.on_unsupported_diff {
            MongoUnsupportedDiffPolicy::Error => Err(err.context(format!(
                "unsupported mongo update, set [extractor] on_unsupported_diff=skip to ignore it, {}",
                context
            ))),
            MongoUnsupportedDiffPolicy::Skip => {
                log_error!("skipping unsupported mongo update: {}, {}", err, context);
                Ok(())
            }
        }
    }

    fn require_doc<'a>(value: &'a Option<&Bson>, field: &str) -> anyhow::Result<&'a Document> {
        value
            .and_then(|v| v.as_document())
            .with_context(|| format!("op_log entry has no valid `{}`", field))
    }

    async fn push_row_to_buf(
        &mut self,
        row_data: RowData,
        position: Position,
    ) -> anyhow::Result<()> {
        if SystemDb::is_system_db(&row_data.schema, &DbType::Mongo) {
            return Ok(());
        }

        if self
            .filter
            .filter_event(&row_data.schema, &row_data.tb, &row_data.row_type)
        {
            self.base_extractor.record_extracted_metrics_row(&row_data);
            return self
                .base_extractor
                .push_dt_data(DtData::Heartbeat {}, position)
                .await;
        }
        self.base_extractor.push_row(row_data, position).await
    }

    fn parse_start_timestamp(&mut self) -> Timestamp {
        let time = if self.start_timestamp > 0 {
            self.start_timestamp
        } else {
            Utc::now().timestamp() as u32
        };
        Timestamp { time, increment: 0 }
    }

    fn start_heartbeat(&mut self, cancel_token: CancellationToken) -> anyhow::Result<()> {
        let db_tb = self.base_extractor.precheck_heartbeat(
            self.heartbeat_interval_secs,
            &self.heartbeat_tb,
            DbType::Mongo,
        );
        if db_tb.len() != 2 {
            return Ok(());
        }

        self.filter.add_ignore_tb(&db_tb[0], &db_tb[1]);

        let (app_name, heartbeat_interval_secs, syncer, mongo_client) = (
            self.app_name.clone(),
            self.heartbeat_interval_secs,
            self.syncer.clone(),
            self.mongo_client.clone(),
        );

        tokio::spawn(async move {
            let mut start_time = Instant::now();
            while !cancel_token.is_cancelled() {
                if start_time.elapsed().as_secs() >= heartbeat_interval_secs {
                    if let Err(err) =
                        Self::heartbeat(&app_name, &db_tb[0], &db_tb[1], &syncer, &mongo_client)
                            .await
                    {
                        log_error!("heartbeat failed: {}", err);
                    }
                    start_time = Instant::now();
                }
                // sleep interruptibly, so shutdown is not delayed by a full heartbeat interval
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    _ = TimeUtil::sleep_millis(1000 * heartbeat_interval_secs) => {}
                }
            }
        });
        log_info!("heartbeat started");
        Ok(())
    }

    async fn heartbeat(
        app_name: &str,
        db: &str,
        tb: &str,
        syncer: &Arc<Mutex<Syncer>>,
        client: &Client,
    ) -> anyhow::Result<()> {
        let (received_resume_token, received_operation_time, received_timestamp) =
            if let Position::MongoCdc {
                resume_token,
                operation_time,
                timestamp,
            } = &syncer.lock().await.received_position
            {
                (
                    resume_token.to_owned(),
                    *operation_time,
                    timestamp.to_owned(),
                )
            } else {
                (String::new(), 0, String::new())
            };
        let (committed_resume_token, committed_operation_time, committed_timestamp) =
            if let Position::MongoCdc {
                resume_token,
                operation_time,
                timestamp,
            } = &syncer.lock().await.committed_position
            {
                (
                    resume_token.to_owned(),
                    *operation_time,
                    timestamp.to_owned(),
                )
            } else {
                (String::new(), 0, String::new())
            };

        let query_doc = doc! {MongoConstants::ID: app_name };
        let update_doc = doc! {MongoConstants::SET: doc! {MongoConstants::ID: app_name,
            "update_timestamp": Position::format_timestamp_millis(Utc::now().timestamp() * 1000),
            "received_resume_token": received_resume_token,
            "received_operation_time": received_operation_time,
            "received_timestamp": received_timestamp,
            "committed_resume_token": committed_resume_token,
            "committed_operation_time": committed_operation_time,
            "committed_timestamp": committed_timestamp,
        }};

        let collection = client.database(db).collection::<Document>(tb);
        let options = UpdateOptions::builder().upsert(true).build();
        if let Err(err) = collection
            .update_one(query_doc, update_doc, Some(options))
            .await
        {
            log_error!("heartbeat failed: {:?}", err);
        }
        Ok(())
    }
}
