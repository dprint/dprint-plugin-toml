use super::Configuration;
use super::IndentKind;
use super::QuoteStyle;
use super::TrailingCommaKind;
use dprint_core::configuration::*;

/// Resolves configuration from a collection of key value strings.
///
/// # Example
///
/// ```
/// use std::collections::HashMap;
/// use dprint_core::configuration::ConfigKeyMap;
/// use dprint_core::configuration::resolve_global_config;
/// use dprint_plugin_toml::configuration::resolve_config;
///
/// let mut config_map = ConfigKeyMap::new(); // get a collection of key value pairs from somewhere
/// let global_config_result = resolve_global_config(&mut config_map);
///
/// // check global_config_result.diagnostics here...
///
/// let config_map = ConfigKeyMap::new(); // get a collection of k/v pairs from somewhere
/// let config_result = resolve_config(
///     config_map,
///     &global_config_result.config
/// );
///
/// // check config_result.diagnostics here and use config_result.config
/// ```
pub fn resolve_config(config: ConfigKeyMap, global_config: &GlobalConfiguration) -> ResolveConfigurationResult<Configuration> {
  let mut diagnostics = Vec::new();
  let mut config = config;

  // general options that the more specific ones below fall back to
  let prefer_single_line = get_value(&mut config, "preferSingleLine", false, &mut diagnostics);

  let resolved_config = Configuration {
    line_width: get_value(
      &mut config,
      "lineWidth",
      global_config.line_width.unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.line_width),
      &mut diagnostics,
    ),
    use_tabs: get_value(
      &mut config,
      "useTabs",
      global_config.use_tabs.unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.use_tabs),
      &mut diagnostics,
    ),
    indent_width: get_value(&mut config, "indentWidth", global_config.indent_width.unwrap_or(2), &mut diagnostics),
    new_line_kind: get_value(
      &mut config,
      "newLineKind",
      global_config.new_line_kind.unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.new_line_kind),
      &mut diagnostics,
    ),
    quote_style: get_value(&mut config, "quoteStyle", QuoteStyle::PreferDouble, &mut diagnostics),
    indent_tables: get_value(&mut config, "indentTables", IndentKind::Maintain, &mut diagnostics),
    indent_entries: get_value(&mut config, "indentEntries", IndentKind::Maintain, &mut diagnostics),
    trailing_commas: get_value(&mut config, "trailingCommas", TrailingCommaKind::OnlyMultiLine, &mut diagnostics),
    space_surrounding_equals: get_value(&mut config, "spaceSurroundingEquals", true, &mut diagnostics),
    sort_keys: get_value(&mut config, "sortKeys", false, &mut diagnostics),
    sort_arrays: get_value(&mut config, "sortArrays", false, &mut diagnostics),
    sort_inline_tables: get_value(&mut config, "sortInlineTables", false, &mut diagnostics),
    array_prefer_single_line: get_value(&mut config, "array.preferSingleLine", prefer_single_line, &mut diagnostics),
    array_space_surrounding_brackets: get_value(&mut config, "array.spaceSurroundingBrackets", false, &mut diagnostics),
    inline_table_prefer_single_line: get_value(&mut config, "inlineTable.preferSingleLine", prefer_single_line, &mut diagnostics),
    inline_table_space_surrounding_braces: get_value(&mut config, "inlineTable.spaceSurroundingBraces", true, &mut diagnostics),
    comment_force_leading_space: get_value(&mut config, "comment.forceLeadingSpace", true, &mut diagnostics),
    cargo_apply_conventions: get_value(&mut config, "cargo.applyConventions", true, &mut diagnostics),
  };

  diagnostics.extend(get_unknown_property_diagnostics(config));

  ResolveConfigurationResult {
    config: resolved_config,
    diagnostics,
  }
}
