use std::path::PathBuf;

use dprint_plugin_toml::configuration::ConfigurationBuilder;
use dprint_plugin_toml::*;

#[test]
fn should_handle_windows_newlines() {
  let config = ConfigurationBuilder::new().build();
  let file_text = format_text(&PathBuf::from("file.toml"), "# 1\r\n# 2\r\n", &config).unwrap();

  assert_eq!(file_text.unwrap(), "# 1\n# 2\n");
}

/// The parser's diagnostics have no spec coverage, since a spec asserts formatted output rather
/// than a failure. A lone carriage return is here because it used to spin forever.
/// Nesting deeply enough to run out of stack aborts the process rather than raising an error, so
/// the parser refuses well before that.
#[test]
fn should_reject_deeply_nested_collections() {
  let config = ConfigurationBuilder::new().build();
  for text in [format!("a = {}", "[".repeat(200)), format!("a = {}", "{ b = ".repeat(200))] {
    let error = format_text(&PathBuf::from("file.toml"), &text, &config).unwrap_err().to_string();
    assert!(error.contains("nested too deeply"), "got {error}");
  }
  // a depth a real document might plausibly reach is still accepted
  let nested = format!("a = {}{}", "[".repeat(16), "]".repeat(16));
  assert!(format_text(&PathBuf::from("file.toml"), &nested, &config).is_ok());
}

#[test]
fn should_report_parse_errors() {
  let config = ConfigurationBuilder::new().build();
  let cases = [
    ("a = 1\rb = 2\n", "expected a line feed after the carriage return"),
    ("a = [1,\r2]\n", "expected a line feed after the carriage return"),
    ("a = { b = 1,\rc = 2 }\n", "expected a line feed after the carriage return"),
    ("b = BAR\n", "expected a value"),
    ("a = \"unterminated\n", "unterminated string"),
    // a line-ending backslash belongs to multi-line strings only
    ("a = \"x\\\ny\"\n", "unterminated string"),
    ("a = [1, 2\n", "unterminated array"),
    ("a = { b = 1\n", "unterminated inline table"),
    ("a = [1 2]\n", "expected ',' between array values"),
    ("a = { b = 1 c = 2 }\n", "expected ',' between inline table entries"),
    ("a\n", "expected '=' after the key"),
    ("[table\n", "expected ']' to close the table header"),
    ("\"\"\"key\"\"\" = 1\n", "a key cannot be a multi-line string"),
    ("a = \"\"\"\"\"\"\"\"\"\"\n", "too many '\"' in a row to end a multi-line string"),
    ("a = 1 b = 2\n", "expected a newline"),
    ("[a] x\n", "expected a newline"),
  ];
  for (input, expected) in cases {
    let error = format_text(&PathBuf::from("file.toml"), input, &config).unwrap_err().to_string();
    assert!(
      error.contains(expected),
      "for {input:?}\n  expected message containing {expected:?}\n  got {error}"
    );
  }
}
