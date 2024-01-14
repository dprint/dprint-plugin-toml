extern crate dprint_development;
extern crate dprint_plugin_toml;

//#[macro_use] extern crate debug_here;

use std::path::PathBuf;
// use std::time::Instant;

use dprint_core::configuration::*;
use dprint_development::*;
use dprint_plugin_toml::configuration::resolve_config;
use dprint_plugin_toml::configuration::ConfigurationBuilder;
use dprint_plugin_toml::*;

#[test]
fn test_specs() {
  //debug_here!();
  let global_config = GlobalConfiguration::default();

  run_specs(
    &PathBuf::from("./tests/specs"),
    &ParseSpecOptions {
      default_file_name: "file.toml",
    },
    &RunSpecsOptions {
      fix_failures: false,
      format_twice: true,
    },
    {
      let global_config = global_config.clone();
      move |file_path, file_text, spec_config| {
        let spec_config: ConfigKeyMap = serde_json::from_value(spec_config.clone().into()).unwrap();
        let config_result = resolve_config(spec_config, &global_config);
        ensure_no_diagnostics(&config_result.diagnostics);

        format_text(file_path, &file_text, &config_result.config)
      }
    },
    move |_file_path, _file_text, _spec_config| {
      #[cfg(feature = "tracing")]
      {
        let config_result = resolve_config(parse_config_key_map(_spec_config), &global_config);
        ensure_no_diagnostics(&config_result.diagnostics);
        return serde_json::to_string(&trace_file(_file_name, _file_text, &config_result.config)).unwrap();
      }

      #[cfg(not(feature = "tracing"))]
      panic!("\n====\nPlease run with `cargo test --features tracing` to get trace output\n====\n")
    },
  )
}

#[test]
fn should_handle_windows_newlines() {
  let config = ConfigurationBuilder::new().build();
  let file_text = format_text(&PathBuf::from("file.toml"), "# 1\r\n# 2\r\n", &config).unwrap();

  assert_eq!(file_text.unwrap(), "# 1\n# 2\n");
}
