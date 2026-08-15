/// The input could not be parsed as TOML.
///
/// The message is a pre-rendered diagnostic that includes the offending line
/// and a caret pointing at the position of the error.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ParseError {
  message: String,
}

impl ParseError {
  pub(crate) fn new(message: String) -> Self {
    ParseError { message }
  }
}

/// An error that can occur while formatting a TOML file.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
  /// The input could not be parsed as TOML.
  #[error(transparent)]
  Parse(#[from] ParseError),
}
