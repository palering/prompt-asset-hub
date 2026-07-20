use pah_domain::DomainError;
use thiserror::Error;

use crate::{BlobStoreError, RepositoryError};

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("{entity} {id} was not found in the current tenant")]
    NotFound { entity: &'static str, id: String },

    #[error(transparent)]
    Repository(#[from] RepositoryError),

    #[error(transparent)]
    BlobStore(#[from] BlobStoreError),
}
