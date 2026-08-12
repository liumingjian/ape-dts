pub struct MongoConstants {}

impl MongoConstants {
    pub const ID: &'static str = "_id";
    pub const DOC: &'static str = "doc";
    pub const DIFF_DOC: &'static str = "diff_doc";
    /// same as DIFF_DOC, but the source doc is already gone, so the target must not be upserted
    pub const DIFF_DOC_NO_UPSERT: &'static str = "diff_doc_no_upsert";
    pub const SET: &'static str = "$set";
    pub const UNSET: &'static str = "$unset";
    pub const PUSH: &'static str = "$push";
}
