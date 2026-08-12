use std::str::FromStr;

use crate::error::Error;
use strum::IntoStaticStr;

/// What to do when an update oplog entry carries a diff that can not be replayed on the target,
/// e.g. an array truncation, whose replay needs the whole array the oplog does not carry.
#[derive(Clone, Copy, IntoStaticStr, Debug, PartialEq)]
pub enum MongoUnsupportedDiffPolicy {
    /// fail the task, the default: a diff we can not replay means the target is about to
    /// silently diverge from the source
    #[strum(serialize = "error")]
    Error,

    /// log the entry and move on, keeping the old (lossy) behaviour
    #[strum(serialize = "skip")]
    Skip,
}

impl FromStr for MongoUnsupportedDiffPolicy {
    type Err = Error;
    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match str {
            "skip" => Ok(Self::Skip),
            "" | "error" => Ok(Self::Error),
            _ => Err(Error::ConfigError(format!(
                "invalid on_unsupported_diff: `{}`, expect `error` or `skip`",
                str
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        assert_eq!(
            MongoUnsupportedDiffPolicy::from_str("").unwrap(),
            MongoUnsupportedDiffPolicy::Error
        );
        assert_eq!(
            MongoUnsupportedDiffPolicy::from_str("error").unwrap(),
            MongoUnsupportedDiffPolicy::Error
        );
        assert_eq!(
            MongoUnsupportedDiffPolicy::from_str("skip").unwrap(),
            MongoUnsupportedDiffPolicy::Skip
        );
        assert!(MongoUnsupportedDiffPolicy::from_str("nope").is_err());
    }
}
