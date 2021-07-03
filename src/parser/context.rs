use std::collections::HashSet;
use crate::configuration::Configuration;

pub struct Context<'a> {
    pub config: &'a Configuration,
    pub text: &'a str,
    pub handled_comments: HashSet<usize>,
}
