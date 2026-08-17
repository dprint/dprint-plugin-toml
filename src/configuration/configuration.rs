use dprint_core::configuration::NewLineKind;
use dprint_core::configuration::ParseConfigurationError;
use dprint_core::generate_str_to_from;
use serde::Deserialize;
use serde::Serialize;

/// Which quote a single-line string is written with.
#[derive(Clone, PartialEq, Eq, Debug, Copy, Serialize, Deserialize)]
pub enum QuoteStyle {
  /// Keeps the quote the string was written with.
  #[serde(rename = "maintain")]
  Maintain,
  /// Uses a double quote when doing so doesn't require adding an escape.
  #[serde(rename = "preferDouble")]
  PreferDouble,
  /// Uses a single quote when doing so doesn't require adding an escape.
  #[serde(rename = "preferSingle")]
  PreferSingle,
}

generate_str_to_from![
  QuoteStyle,
  [Maintain, "maintain"],
  [PreferDouble, "preferDouble"],
  [PreferSingle, "preferSingle"]
];

/// Whether something is indented.
#[derive(Clone, PartialEq, Eq, Debug, Copy, Serialize, Deserialize)]
pub enum IndentKind {
  /// Indents where the file already did.
  #[serde(rename = "maintain")]
  Maintain,
  /// Always indents.
  #[serde(rename = "always")]
  Always,
  /// Never indents.
  #[serde(rename = "never")]
  Never,
}

generate_str_to_from![IndentKind, [Maintain, "maintain"], [Always, "always"], [Never, "never"]];

/// When a trailing comma is written after the last value of an array or inline table.
#[derive(Clone, PartialEq, Eq, Debug, Copy, Serialize, Deserialize)]
pub enum TrailingCommaKind {
  /// Writes one only when the array or inline table is written over multiple lines.
  #[serde(rename = "onlyMultiLine")]
  OnlyMultiLine,
  /// Never writes one.
  #[serde(rename = "never")]
  Never,
}

generate_str_to_from![TrailingCommaKind, [OnlyMultiLine, "onlyMultiLine"], [Never, "never"]];

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
  pub line_width: u32,
  pub use_tabs: bool,
  pub indent_width: u8,
  pub new_line_kind: NewLineKind,
  pub quote_style: QuoteStyle,
  pub indent_tables: IndentKind,
  pub indent_entries: IndentKind,
  pub trailing_commas: TrailingCommaKind,
  pub space_surrounding_equals: bool,
  pub sort_keys: bool,
  pub sort_arrays: bool,
  pub sort_inline_tables: bool,
  pub array_prefer_single_line: bool,
  pub array_space_surrounding_brackets: bool,
  pub inline_table_prefer_single_line: bool,
  pub inline_table_space_surrounding_braces: bool,
  pub comment_force_leading_space: bool,
  pub cargo_apply_conventions: bool,
}
