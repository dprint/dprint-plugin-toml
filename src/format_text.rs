use super::configuration::Configuration;
use super::generation::generate;
use crate::cargo;

use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::PrintOptions;
use std::path::Path;
use taplo::syntax::SyntaxNode;

pub fn format_text(file_path: &Path, text: &str, config: &Configuration) -> Result<Option<String>> {
  let result = format_text_inner(file_path, text, config)?;
  if result == text {
    Ok(None)
  } else {
    Ok(Some(result))
  }
}

fn format_text_inner(file_path: &Path, text: &str, config: &Configuration) -> Result<String> {
  let text = strip_bom(text);
  let node = parse_and_process_node(file_path, text, config)?;

  Ok(dprint_core::formatting::format(
    || generate(node, text, config),
    config_to_print_options(text, config),
  ))
}

#[cfg(feature = "tracing")]
pub fn trace_file(file_path: &Path, text: &str, config: &Configuration) -> dprint_core::formatting::TracingResult {
  let node = parse_and_process_node(file_path, text, config).unwrap();

  dprint_core::formatting::trace_printing(|| generate(node, text, config), config_to_print_options(text, config))
}

fn strip_bom(text: &str) -> &str {
  text.strip_prefix("\u{FEFF}").unwrap_or(text)
}

fn parse_and_process_node(file_path: &Path, text: &str, config: &Configuration) -> Result<SyntaxNode> {
  let node = parse_taplo(text)?;

  Ok(if config.cargo_apply_conventions && cargo::is_cargo_toml_file(file_path) {
    cargo::apply_cargo_toml_conventions(node)
  } else {
    node
  })
}

fn parse_taplo(text: &str) -> Result<SyntaxNode> {
  let parse_result = taplo::parser::parse(text);

  if let Some(err) = parse_result.errors.first() {
    bail!(
      "{}",
      dprint_core::formatting::utils::string_utils::format_diagnostic(Some((err.range.start().into(), err.range.end().into())), &err.message, text,)
    )
  } else {
    Ok(parse_result.into_syntax())
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
