use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum LctrlError {
    #[error("feature is unsupported: {feature}")]
    Unsupported { feature: String },
    #[error("channel is unavailable: {channel}")]
    ChannelUnavailable { channel: String },
    #[error("permission denied; requires {need}")]
    PermissionDenied { need: String },
    #[error("firmware rejected request: {detail}")]
    FirmwareRejected { detail: String },
    #[error("invalid argument: {detail}")]
    InvalidArgument { detail: String },
    #[error("readback mismatch: requested {requested}, actual {actual}")]
    VerifyMismatch { requested: String, actual: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl LctrlError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Io(_) => 1,
            Self::InvalidArgument { .. } => 2,
            Self::Unsupported { .. } => 3,
            Self::ChannelUnavailable { .. } => 4,
            Self::PermissionDenied { .. } => 5,
            Self::FirmwareRejected { .. } => 6,
            Self::VerifyMismatch { .. } => 7,
        }
    }

    #[must_use]
    pub fn report(&self) -> ErrorReport {
        let (kind, context) = match self {
            Self::Unsupported { feature } => (
                "unsupported",
                BTreeMap::from([("feature", feature.clone())]),
            ),
            Self::ChannelUnavailable { channel } => (
                "channel_unavailable",
                BTreeMap::from([("channel", channel.clone())]),
            ),
            Self::PermissionDenied { need } => (
                "permission_denied",
                BTreeMap::from([("need", need.clone())]),
            ),
            Self::FirmwareRejected { detail } => (
                "firmware_rejected",
                BTreeMap::from([("detail", detail.clone())]),
            ),
            Self::InvalidArgument { detail } => (
                "invalid_argument",
                BTreeMap::from([("detail", detail.clone())]),
            ),
            Self::VerifyMismatch { requested, actual } => (
                "verify_mismatch",
                BTreeMap::from([("actual", actual.clone()), ("requested", requested.clone())]),
            ),
            Self::Io(_) => ("io", BTreeMap::new()),
        };

        ErrorReport {
            error: ErrorBody {
                kind,
                message: self.to_string(),
                context,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorReport {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    kind: &'static str,
    message: String,
    #[serde(flatten)]
    context: BTreeMap<&'static str, String>,
}

pub type Result<T> = std::result::Result<T, LctrlError>;
