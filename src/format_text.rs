use super::configuration::Configuration;
use super::generation::generate;
use crate::cargo;

use crate::ast::Root;
use crate::error::FormatError;
use crate::error::ParseError;
use crate::parser;

use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::PrintOptions;
use std::path::Path;

pub fn format_text(file_path: &Path, text: &str, config: &Configuration) -> Result<Option<String>, FormatError> {
  let result = format_text_inner(file_path, text, config)?;
  if result == text {
    Ok(None)
  } else {
    Ok(Some(result))
  }
}

fn format_text_inner(file_path: &Path, text: &str, config: &Configuration) -> Result<String, FormatError> {
  let text = strip_bom(text);
  let root = parse_and_process_node(file_path, text, config)?;

  Ok(dprint_core::formatting::format(
    || generate(&root, config),
    config_to_print_options(text, config),
  ))
}

#[cfg(feature = "tracing")]
pub fn trace_file(file_path: &Path, text: &str, config: &Configuration) -> dprint_core::formatting::TracingResult {
  let root = parse_and_process_node(file_path, text, config).unwrap();

  dprint_core::formatting::trace_printing(|| generate(&root, config), config_to_print_options(text, config))
}

fn strip_bom(text: &str) -> &str {
  text.strip_prefix("\u{FEFF}").unwrap_or(text)
}

fn parse_and_process_node<'a>(file_path: &Path, text: &'a str, config: &Configuration) -> Result<Root<'a>, FormatError> {
  let mut root = parse(text)?;

  crate::sorting::apply_sorting(&mut root, config);

  // after the general sorting, so that a Cargo.toml keeps its conventional order rather than an
  // alphabetical one
  if config.cargo_apply_conventions && cargo::is_cargo_toml_file(file_path) {
    cargo::apply_cargo_toml_conventions(&mut root);
  }
  Ok(root)
}

fn parse(text: &str) -> Result<Root<'_>, ParseError> {
  parser::parse(text).map_err(|err| {
    let (start, end) = highlight_range(&err, text);
    ParseError::new(dprint_core::formatting::utils::string_utils::format_diagnostic(
      Some((start, end)),
      &err.message,
      text,
    ))
  })
}

/// The range to underline beneath the offending line.
///
/// The diagnostic only underlines within a single line, and draws nothing at all for an empty range
/// or one that runs past the end of its line — which is exactly what an error at the end of the
/// input or inside an unterminated string produces. Keeping the range on its own line, and at least
/// one character wide, means every error gets a caret.
fn highlight_range(err: &parser::SyntaxError, text: &str) -> (usize, usize) {
  let line_end = text[err.span.start..].find('\n').map(|i| err.span.start + i).unwrap_or(text.len());
  let end = err.span.end.min(line_end);
  if end > err.span.start {
    (err.span.start, end)
  } else {
    // an empty range still needs something to point at: the character it sits before, or the one
    // it sits after when there is nothing left in the input
    let start = err.span.start.min(text.len());
    match text[start..].chars().next() {
      Some(c) => (start, start + c.len_utf8()),
      None => (text[..start].chars().next_back().map(|c| start - c.len_utf8()).unwrap_or(start), start),
    }
  }
}

fn config_to_print_options(text: &str, config: &Configuration) -> PrintOptions {
  PrintOptions {
    indent_width: config.indent_width,
    max_width: config.line_width,
    use_tabs: config.use_tabs,
    new_line_text: resolve_new_line_kind(text, config.new_line_kind),
  }
}

#[cfg(test)]
mod test {
  #[test]
  fn strips_bom() {
    let config = crate::configuration::ConfigurationBuilder::new().build();
    let file_text = crate::format_text::format_text(&std::path::PathBuf::from("file.toml"), "\u{FEFF}# 1\n# 2\n", &config).unwrap();

    assert_eq!(file_text.unwrap(), "# 1\n# 2\n");
  }
}
