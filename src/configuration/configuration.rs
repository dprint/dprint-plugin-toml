use dprint_core::configuration::NewLineKind;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
  pub line_width: u32,
  pub use_tabs: bool,
  pub indent_width: u8,
  pub new_line_kind: NewLineKind,
  pub comment_force_leading_space: bool,
  pub cargo_apply_conventions: bool,
}
