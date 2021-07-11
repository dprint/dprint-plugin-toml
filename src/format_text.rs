use super::configuration::Configuration;
use super::parser::parse_items;
use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::PrintOptions;
use dprint_core::types::ErrBox;
use std::path::Path;
use taplo::syntax::SyntaxNode;

pub fn format_text(
    _file_path: &Path,
    text: &str,
    config: &Configuration,
) -> Result<String, ErrBox> {
    let node = parse_taplo(text)?;

    Ok(dprint_core::formatting::format(
        || parse_items(node, text, config),
        config_to_print_options(text, config),
    ))
}

#[cfg(feature = "tracing")]
pub fn trace_file(text: &str, config: &Configuration) -> dprint_core::formatting::TracingResult {
    let node = parse_taplo(text).unwrap();

    dprint_core::formatting::trace_printing(
        || parse_items(node, text, config),
        config_to_print_options(text, config),
    )
}

fn parse_taplo(text: &str) -> Result<SyntaxNode, String> {
    let parse_result = taplo::parser::parse(text);

    if let Some(err) = parse_result.errors.get(0) {
        Err(
            dprint_core::formatting::utils::string_utils::format_diagnostic(
                Some((err.range.start().into(), err.range.end().into())),
                &err.message,
                text,
            ),
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
