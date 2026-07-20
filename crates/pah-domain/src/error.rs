use thiserror::Error;

/// A violation of a domain invariant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DomainError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },

    #[error("invalid {field}: {reason}")]
    InvalidValue { field: &'static str, reason: String },

    #[error("score for {field} must be between 1 and 5, got {value}")]
    ScoreOutOfRange { field: String, value: u8 },

    #[error("invalid {entity} state transition from {from} to {to}")]
    InvalidStateTransition {
        entity: &'static str,
        from: String,
        to: String,
    },

    #[error("at least one asset location must be supplied")]
    MissingAssetLocation,

    #[error("asset link has no owning context")]
    MissingAssetLinkContext,

    #[error("timestamp {field} cannot be earlier than {earlier_field}")]
    InvalidTimestampOrder {
        field: &'static str,
        earlier_field: &'static str,
    },
}
