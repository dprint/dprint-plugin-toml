use crate::configuration::Configuration;

pub struct Context<'a> {
  pub config: &'a Configuration,
  /// How many single-line inline tables enclose whatever is being generated. A table within one is
  /// kept on a single line too, since a newline between its braces would not be within a value.
  single_line_table_depth: usize,
}

impl<'a> Context<'a> {
  pub fn new(config: &'a Configuration) -> Self {
    Self {
      config,
      single_line_table_depth: 0,
    }
  }

  pub fn is_in_single_line_table(&self) -> bool {
    self.single_line_table_depth > 0
  }

  pub fn with_single_line_table<T>(&mut self, action: impl FnOnce(&mut Self) -> T) -> T {
    self.single_line_table_depth += 1;
    let result = action(self);
    self.single_line_table_depth -= 1;
    result
  }
}
