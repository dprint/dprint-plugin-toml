use std::collections::HashSet;
use crate::configuration::Configuration;

pub struct Context<'a> {
    pub config: &'a Configuration,
    pub text: &'a str,
    handled_comments: HashSet<usize>,
}

impl<'a> Context<'a> {
    pub fn new(text: &'a str, config: &'a Configuration) -> Self {
        Self {
            config,
            text,
            handled_comments: HashSet::new(),
        }
    }

    pub fn has_handled_comment(&self, pos: usize) -> bool {
        self.handled_comments.contains(&pos)
    }

    pub fn add_handled_comment(&mut self, pos: usize) {
        self.handled_comments.insert(pos);
    }
}
