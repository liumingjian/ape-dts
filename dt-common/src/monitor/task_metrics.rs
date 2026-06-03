use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

#[derive(
    PartialOrd,
    Ord,
    EnumString,
    IntoStaticStr,
    Display,
    PartialEq,
    Eq,
    Hash,
    Clone,
    Copy,
    Debug,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TaskMetricsType {
    Lag,
    Timestamp,
    Progress,
    TotalProgressCount,
    FinishedProgressCount,

    // describe the overall traffic before filtering
    // TODO: some traffic need to be decoded first, e.g., sqlx row data which fields not directly map to dt row data, which need to track the size of tcp stream
    ExtractorRpsMax,
    ExtractorRpsMin,
    ExtractorRpsAvg,
    ExtractorBpsMax,
    ExtractorBpsMin,
    ExtractorBpsAvg,

    ExtractorPlanRecords,

    // describe the overall traffic after filtering
    ExtractorPushedRpsMax,
    ExtractorPushedRpsMin,
    ExtractorPushedRpsAvg,
    ExtractorPushedBpsMax,
    ExtractorPushedBpsMin,
    ExtractorPushedBpsAvg,

    PipelineQueueSize,
    PipelineQueueBytes,

    PipelineRecordSizeMax,

    SinkerRtMax,
    SinkerRtMin,
    SinkerRtAvg,

    SinkerRpsMax,
    SinkerRpsMin,
    SinkerRpsAvg,
    SinkerBpsMax,
    SinkerBpsMin,
    SinkerBpsAvg,

    SinkerSinkedRecords,
    SinkerSinkedBytes,

    SinkerDdlCount,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_serializes_to_lag_string() {
        let json = serde_json::to_string(&TaskMetricsType::Lag).unwrap();
        assert_eq!(json, "\"lag\"");
    }

    #[test]
    fn lag_deserializes_from_lag_string() {
        let v: TaskMetricsType = serde_json::from_str("\"lag\"").unwrap();
        assert_eq!(v, TaskMetricsType::Lag);
    }
}
