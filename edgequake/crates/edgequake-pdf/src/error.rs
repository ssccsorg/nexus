use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdfConversionError {
    #[error("backend not configured: {0}")]
    BackendNotConfigured(&'static str),
    #[error("backend error: {0}")]
    Backend(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("empty output: {0}")]
    EmptyOutput(&'static str),
}
