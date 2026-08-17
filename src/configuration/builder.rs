use dprint_core::configuration::ConfigKeyMap;
use dprint_core::configuration::ConfigKeyValue;
use dprint_core::configuration::GlobalConfiguration;
use dprint_core::configuration::NewLineKind;

use super::*;

/// Formatting configuration builder.
///
/// # Example
///
/// ```
/// use dprint_plugin_toml::configuration::*;
///
/// let config = ConfigurationBuilder::new()
///     .line_width(80)
///     .build();
/// ```
#[derive(Default)]
pub struct ConfigurationBuilder {
  pub(super) config: ConfigKeyMap,
  global_config: Option<GlobalConfiguration>,
}

impl ConfigurationBuilder {
  /// Constructs a new configuration builder.
  pub fn new() -> ConfigurationBuilder {
    ConfigurationBuilder::default()
  }

  /// Gets the final configuration that can be used to format a file.
  pub fn build(&self) -> Configuration {
    if let Some(global_config) = &self.global_config {
      resolve_config(self.config.clone(), global_config).config
    } else {
      let global_config = GlobalConfiguration::default();
      resolve_config(self.config.clone(), &global_config).config
    }
  }

  /// Set the global configuration.
  pub fn global_config(&mut self, global_config: GlobalConfiguration) -> &mut Self {
    self.global_config = Some(global_config);
    self
  }

  /// The width of a line the printer will try to stay under. Note that the printer may exceed this width in certain cases.
  /// Default: 120
  pub fn line_width(&mut self, value: u32) -> &mut Self {
    self.insert("lineWidth", (value as i32).into())
  }

  /// Whether to use tabs (true) or spaces (false).
  ///
  /// Default: `false`
  pub fn use_tabs(&mut self, value: bool) -> &mut Self {
    self.insert("useTabs", value.into())
  }

  /// The number of columns for an indent.
  ///
  /// Default: `2`
  pub fn indent_width(&mut self, value: u8) -> &mut Self {
    self.insert("indentWidth", (value as i32).into())
  }

  /// The kind of newline to use.
  /// Default: `NewLineKind::LineFeed`
  pub fn new_line_kind(&mut self, value: NewLineKind) -> &mut Self {
    self.insert("newLineKind", value.to_string().into())
  }

  /// Which quote to write a single-line string with.
  ///
  /// Default: `QuoteStyle::PreferDouble`
  pub fn quote_style(&mut self, value: QuoteStyle) -> &mut Self {
    self.insert("quoteStyle", value.to_string().into())
  }

  /// Whether to indent a table header that is a subtable of the one before it.
  ///
  /// Default: `IndentKind::Maintain`
  pub fn indent_tables(&mut self, value: IndentKind) -> &mut Self {
    self.insert("indentTables", value.to_string().into())
  }

  /// Whether to indent the entries of a table beneath its header.
  ///
  /// Default: `IndentKind::Maintain`
  pub fn indent_entries(&mut self, value: IndentKind) -> &mut Self {
    self.insert("indentEntries", value.to_string().into())
  }

  /// When to write a trailing comma after the last value of an array or inline table.
  ///
  /// Default: `TrailingCommaKind::OnlyMultiLine`
  pub fn trailing_commas(&mut self, value: TrailingCommaKind) -> &mut Self {
    self.insert("trailingCommas", value.to_string().into())
  }

  /// Whether to write a space on either side of the `=` of a key/value pair.
  ///
  /// Default: `true`
  pub fn space_surrounding_equals(&mut self, value: bool) -> &mut Self {
    self.insert("spaceSurroundingEquals", value.into())
  }

  /// Whether to alphabetically sort the entries of a table.
  ///
  /// Default: `false`
  pub fn sort_keys(&mut self, value: bool) -> &mut Self {
    self.insert("sortKeys", value.into())
  }

  /// Whether to alphabetically sort the values of an array.
  ///
  /// Default: `false`
  pub fn sort_arrays(&mut self, value: bool) -> &mut Self {
    self.insert("sortArrays", value.into())
  }

  /// Whether to alphabetically sort the entries of an inline table.
  ///
  /// Default: `false`
  pub fn sort_inline_tables(&mut self, value: bool) -> &mut Self {
    self.insert("sortInlineTables", value.into())
  }

  /// Whether to collapse an array or inline table onto a single line when it fits, even when it
  /// was written over several lines.
  ///
  /// Default: `false`
  pub fn prefer_single_line(&mut self, value: bool) -> &mut Self {
    self.insert("preferSingleLine", value.into())
  }

  /// Whether to collapse an array onto a single line when it fits, even when it was written over
  /// several lines.
  ///
  /// Default: `false`
  pub fn array_prefer_single_line(&mut self, value: bool) -> &mut Self {
    self.insert("array.preferSingleLine", value.into())
  }

  /// Whether to write a space inside the brackets of a single-line array.
  ///
  /// Default: `false`
  pub fn array_space_surrounding_brackets(&mut self, value: bool) -> &mut Self {
    self.insert("array.spaceSurroundingBrackets", value.into())
  }

  /// Whether to collapse an inline table onto a single line when it was written over several
  /// lines.
  ///
  /// Default: `false`
  pub fn inline_table_prefer_single_line(&mut self, value: bool) -> &mut Self {
    self.insert("inlineTable.preferSingleLine", value.into())
  }

  /// Whether to write a space inside the braces of a single-line inline table.
  ///
  /// Default: `true`
  pub fn inline_table_space_surrounding_braces(&mut self, value: bool) -> &mut Self {
    self.insert("inlineTable.spaceSurroundingBraces", value.into())
  }

  /// Forces a leading space after the hashes.
  /// Default: `true`
  pub fn comment_force_leading_space(&mut self, value: bool) -> &mut Self {
    self.insert("comment.forceLeadingSpace", value.into())
  }

  /// Whether to apply sorting to a Cargo.toml file.
  /// Default: `true`
  pub fn cargo_apply_conventions(&mut self, value: bool) -> &mut Self {
    self.insert("cargo.applyConventions", value.into())
  }

  #[cfg(test)]
  pub(super) fn get_inner_config(&self) -> ConfigKeyMap {
    self.config.clone()
  }

  fn insert(&mut self, name: &str, value: ConfigKeyValue) -> &mut Self {
    self.config.insert(String::from(name), value);
    self
  }
}

#[cfg(test)]
mod tests {
  use dprint_core::configuration::resolve_global_config;
  use dprint_core::configuration::NewLineKind;

  use super::*;

  #[test]
  fn check_all_values_set() {
    let mut config = ConfigurationBuilder::new();
    config
      .new_line_kind(NewLineKind::CarriageReturnLineFeed)
      .line_width(90)
      .use_tabs(true)
      .indent_width(4)
      .new_line_kind(NewLineKind::CarriageReturnLineFeed)
      .quote_style(QuoteStyle::Maintain)
      .indent_tables(IndentKind::Always)
      .indent_entries(IndentKind::Always)
      .trailing_commas(TrailingCommaKind::Never)
      .space_surrounding_equals(false)
      .sort_keys(true)
      .sort_arrays(true)
      .sort_inline_tables(true)
      .prefer_single_line(true)
      .array_prefer_single_line(true)
      .array_space_surrounding_brackets(true)
      .inline_table_prefer_single_line(true)
      .inline_table_space_surrounding_braces(false)
      .comment_force_leading_space(false)
      .cargo_apply_conventions(false);

    let inner_config = config.get_inner_config();
    assert_eq!(inner_config.len(), 19);
    let diagnostics = resolve_config(inner_config, &Default::default()).diagnostics;
    assert_eq!(diagnostics.len(), 0);
  }

  #[test]
  fn handle_global_config() {
    let mut global_config = ConfigKeyMap::new();
    global_config.insert(String::from("lineWidth"), 90.into());
    global_config.insert(String::from("newLineKind"), "crlf".into());
    global_config.insert(String::from("useTabs"), true.into());
    let global_config = resolve_global_config(&mut global_config).config;
    let mut config_builder = ConfigurationBuilder::new();
    let config = config_builder.global_config(global_config).build();
    assert_eq!(config.line_width, 90);
    assert!(config.new_line_kind == NewLineKind::CarriageReturnLineFeed);
  }

  #[test]
  fn use_defaults_when_global_not_set() {
    let global_config = GlobalConfiguration::default();
    let mut config_builder = ConfigurationBuilder::new();
    let config = config_builder.global_config(global_config).build();
    assert_eq!(config.indent_width, 2); // this is different
    assert!(config.new_line_kind == NewLineKind::LineFeed);
  }
}
