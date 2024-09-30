use std::path::PathBuf;

use dprint_plugin_toml::configuration::ConfigurationBuilder;
use dprint_plugin_toml::*;

#[test]
fn should_handle_windows_newlines() {
  let config = ConfigurationBuilder::new().build();
  let file_text = format_text(&PathBuf::from("file.toml"), "# 1\r\n# 2\r\n", &config).unwrap();

  assert_eq!(file_text.unwrap(), "# 1\n# 2\n");
}
