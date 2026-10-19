use thiserror::Error;

#[derive(Debug, Error)]
pub enum StandaloneError {
    #[error("storage error: {0}")]
    Storage(#[from] sies_storage::StorageError),

    #[error("rebuild error: {0}")]
    Rebuild(#[from] sies_chain::rebuilder::RebuildError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
