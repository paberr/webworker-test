use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Function not found: {0}")]
    FnNotFound(&'static str),
    #[error("WebWorker was lost")]
    WorkerLost,
}

#[derive(Debug, Error)]
pub enum TryRunError {
    #[error("Ran with error: {0}")]
    Inner(#[from] Error),
    #[error("WebWorker capacity reached")]
    Full,
}
