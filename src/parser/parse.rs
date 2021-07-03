use dprint_core::formatting::*;
use dprint_core::formatting::conditions::*;
use dprint_core::formatting::parser_helpers::MultiLineOptions;
use dprint_core::formatting::parser_helpers::parse_raw_string;
use dprint_core::formatting::parser_helpers::parse_raw_string_trim_line_ends;
use dprint_core::types::ErrBox;
use taplo::rowan::SyntaxNodeChildren;
use taplo::syntax::Lang;
use taplo::syntax::{SyntaxNode, SyntaxToken, SyntaxElement, SyntaxKind};
use taplo::rowan::NodeOrToken;
use std::collections::HashSet;

use crate::configuration::Configuration;
use super::Context;

type PrintItemsResult = Result<PrintItems, ()>;

pub fn parse_items(node: SyntaxNode, text: &str, config: &Configuration) -> PrintItems {
    let mut context = Context::new(text, config);
    let mut items = parse_node(node.into(), &mut context);
    items.push_condition(if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        Signal::NewLine.into()
    ));
    items
}

fn parse_node<'a>(node: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
    parse_node_with_inner(node, context, |items, _| items)
}

fn parse_node_with_inner<'a>(
    node: SyntaxElement,
    context: &mut Context<'a>,
    inner_parse: impl FnOnce(PrintItems, &mut Context<'a>) -> PrintItems,
) -> PrintItems {
    let mut items = PrintItems::new();
    println!("{:?}", node);

    let result = match node.clone() {
        NodeOrToken::Node(node) => match node.kind() {
            SyntaxKind::ROOT => parse_root(node, context),
            SyntaxKind::ARRAY => parse_array(node, context),
            SyntaxKind::ENTRY => parse_entry(node, context),
            SyntaxKind::KEY => parse_key(node, context),
            SyntaxKind::VALUE => parse_value(node, context),
            _ => Err(()),
        },
        NodeOrToken::Token(token) => Ok(token.text().as_str().into()),
    };

    items.extend(inner_parse(match result {
        Ok(items) => items,
        Err(()) => parse_raw_string_trim_line_ends(node.text().trim().into()),
    }, context));

    if node.kind() == SyntaxKind::VALUE {
        for comment in node.child_comments() {
            items.extend(parse_comment(comment, context));
        }
    }
    if node.parent().is_none() || node.parent().unwrap().kind() != SyntaxKind::VALUE || !node.is_last_non_trivia_sibling() {
        if let Some(trailing_comment) = get_trailing_comment(node) {
            items.extend(parse_comment(trailing_comment, context));
        }
    }

    items
}

fn parse_root<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    print_formatted_tree(node.clone());

    let mut items = PrintItems::new();
    for (i, child) in node.children().enumerate() {
        if i > 0 {
            items.push_signal(Signal::NewLine);
        }
        items.extend(parse_node(child.into(), context));
    }

    Ok(items)
}

fn parse_array<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let values = node.children();
    let open_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_START)?;
    let close_token = get_token_with_kind(node.clone(), SyntaxKind::BRACKET_END)?;
    ensure_all_kind(values.clone(), SyntaxKind::VALUE)?;

    Ok(parse_surrounded_by_tokens(
        |context| parse_comma_separated_values(ParseCommaSeparatedValuesOptions {
            nodes: values.collect::<Vec<_>>(),
            prefer_hanging: false,
            force_use_new_lines: false,
            allow_blank_lines: true,
            single_line_space_at_start: false,
            single_line_space_at_end: false,
            custom_single_line_separator: None,
            multi_line_options: MultiLineOptions::surround_newlines_indented(),
            force_possible_newline_at_start: false,
        }, context),
        ParseSurroundedByTokensParams {
            open_token,
            close_token,
        },
        context
    ))
}

fn parse_entry<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
    let value = get_child_with_kind(node.clone(), SyntaxKind::VALUE)?;
    let mut items = PrintItems::new();

    items.extend(parse_node(key.into(), context));
    items.push_str(" = ");
    items.extend(parse_node(value.into(), context));

    Ok(items)
}

fn parse_key<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: A key may be either bare, quoted, or dotted.
    Ok(parse_children_inline(node, context))
}

fn parse_value<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    // Spec: Values must be either String, Integer, Float, Boolean, DateTimes, Array, InlineTable
    Ok(parse_children_inline(node, context))
}

fn parse_children_inline<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    for element in get_children_with_non_trivia_tokens(node) {
        items.extend(parse_node(element, context));
    }
    items
}

struct ParseSurroundedByTokensParams {
    open_token: SyntaxToken,
    close_token: SyntaxToken,
}

fn parse_surrounded_by_tokens<'a, 'b>(
    parse_inner: impl FnOnce(&mut Context<'a>) -> PrintItems,
    opts: ParseSurroundedByTokensParams,
    context: &mut Context<'a>
) -> PrintItems {
    // parse
    let mut items = PrintItems::new();
    items.extend(parse_node(opts.open_token.clone().into(), context));

    for comment in get_comments_on_next_lines(opts.open_token.clone().into()) {
        items.push_signal(Signal::NewLine);
        items.extend(parser_helpers::with_indent(parse_comment(comment, context)));
    }

    items.extend(parse_inner(context));

    let before_trailing_comments_info = Info::new("beforeTrailingComments");
    items.push_info(before_trailing_comments_info);
    // todo: trailing comments on different lines
    // items.extend(parser_helpers::with_indent(parse_trailing_comments_as_statements(&open_token_end, context)));
    // if let Some(leading_comments) = context.comments.get(&close_token_start.start()) {
    //    items.extend(parser_helpers::with_indent(parse_comments_as_statements(leading_comments.iter(), None, context)));
    // }
    items.push_condition(conditions::if_true(
        "newLineIfHasCommentsAndNotStartOfNewLine",
        move |context| {
            let had_comments = !condition_resolvers::is_at_same_position(context, &before_trailing_comments_info)?;
            return Some(had_comments && !context.writer_info.is_start_of_line())
        },
        Signal::NewLine.into()
    ));

    items.extend(parse_node(opts.close_token.into(), context));
    items
}

struct ParseCommaSeparatedValuesOptions {
    nodes: Vec<SyntaxNode>,
    prefer_hanging: bool,
    force_use_new_lines: bool,
    allow_blank_lines: bool,
    single_line_space_at_start: bool,
    single_line_space_at_end: bool,
    custom_single_line_separator: Option<PrintItems>,
    multi_line_options: parser_helpers::MultiLineOptions,
    force_possible_newline_at_start: bool,
}

fn parse_comma_separated_values<'a>(
    opts: ParseCommaSeparatedValuesOptions,
    context: &mut Context<'a>
) -> PrintItems {
    let nodes = opts.nodes;
    let indent_width = context.config.indent_width;
    let compute_lines_span = opts.allow_blank_lines && opts.force_use_new_lines; // save time otherwise
    parser_helpers::parse_separated_values(|is_multi_line_ref| {
        let mut parsed_nodes = Vec::new();
        let nodes_count = nodes.len();
        for (i, value) in nodes.into_iter().enumerate() {
            let (allow_inline_multi_line, allow_inline_single_line) = (value.kind() == SyntaxKind::INLINE_TABLE, false);
            let lines_span = None; /*if compute_lines_span {
                value.as_ref().map(|x| parser_helpers::LinesSpan{
                    start_line: context.start_line_with_comments(x),
                    end_line: context.end_line_with_comments(x),
                })
            } else { None };*/
            let items = parser_helpers::new_line_group({
                let parsed_comma = if i == nodes_count - 1 {
                    // todo: make this conditional based on config
                    let is_multi_line = is_multi_line_ref.create_resolver();
                    if_true(
                        "commaIfMultiLine",
                        is_multi_line,
                        ",".into(),
                    ).into()
                } else {
                    ",".into()
                };
                parse_comma_separated_value(value, parsed_comma, context)
            });
            parsed_nodes.push(parser_helpers::ParsedValue {
                items,
                lines_span,
                allow_inline_multi_line,
                allow_inline_single_line,
            });
        }

        parsed_nodes
    }, parser_helpers::ParseSeparatedValuesOptions {
        prefer_hanging: opts.prefer_hanging,
        force_use_new_lines: opts.force_use_new_lines,
        allow_blank_lines: opts.allow_blank_lines,
        single_line_space_at_start: opts.single_line_space_at_start,
        single_line_space_at_end: opts.single_line_space_at_end,
        single_line_separator: opts.custom_single_line_separator.unwrap_or(Signal::SpaceOrNewLine.into()),
        indent_width,
        multi_line_options: opts.multi_line_options,
        force_possible_newline_at_start: opts.force_possible_newline_at_start,
    }).items
}

fn parse_comma_separated_value<'a>(value: SyntaxNode, parsed_comma: PrintItems, context: &mut Context<'a>) -> PrintItems {
    let mut items = PrintItems::new();
    let comma_token = get_next_comma_sibling(value.clone().into());

    let parsed_comma = parsed_comma.into_rc_path();
    items.extend(parse_node_with_inner(value.into(), context, move |mut items, _| {
        // this Rc clone is necessary because we can't move the captured parsed_comma out of this closure
        items.push_optional_path(parsed_comma.clone());
        items
    }));

    // get the trailing comments after the comma token
    if let Some(comma_token) = comma_token {
        items.extend(parse_trailing_comments(comma_token.into(), context));
    }

    items
}

fn parse_trailing_comments<'a>(element: SyntaxElement, context: &mut Context<'a>) -> PrintItems {
    match get_trailing_comment(element) {
        Some(comment) => parse_comment(comment, context),
        None => PrintItems::new(),
    }
}

fn parse_comment<'a>(comment: SyntaxToken, context: &mut Context<'a>) -> PrintItems {
    let pos = comment.text_range().start().into();
    if context.has_handled_comment(pos) {
        return PrintItems::new();
    }
    context.add_handled_comment(pos);

    debug_assert_kind(comment.clone().into(), SyntaxKind::COMMENT);
    let mut items = PrintItems::new();
    items.push_condition(if_false(
        "spaceIfNotStartOfLine",
        |context| Some(condition_resolvers::is_start_of_line(context)),
        " ".into(),
    ));
    items.extend(parse_raw_string(comment.text().as_str()));
    items.push_signal(Signal::ExpectNewLine);
    items
}

fn print_formatted_tree(node: SyntaxNode) {
    print_node_and_children(node, 0);

    fn print_node_and_children(node: SyntaxNode, indent: usize) {
        println!("{}{:?}", " ".repeat(indent), node);
        for c in node.children_with_tokens() {
            match c {
                NodeOrToken::Node(c) => {
                    print_node_and_children(c.clone(), indent + 2);
                }
                NodeOrToken::Token(t) => {
                    println!("{}{:?} [TOKEN]", " ".repeat(indent + 2), t);
                }
            }
        }
    }
}

fn ensure_all_kind(nodes: impl Iterator<Item=SyntaxNode>, kind: SyntaxKind) -> Result<(), ()> {
    for node in nodes {
        ensure_kind(node.into(), kind)?;
    }
    Ok(())
}

fn ensure_kind(element: SyntaxElement, kind: SyntaxKind) -> Result<(), ()> {
    if element.kind() == kind {
        Ok(())
    } else {
        Err(())
    }
}

#[cfg(debug_assertions)]
fn debug_assert_kind(element: SyntaxElement, kind: SyntaxKind) {
    if element.kind() != kind {
        panic!("Debug Assertion: Expected kind {:?}, but was {:?}", kind, element.kind());
    }
}

fn get_next_comma_sibling(mut element: SyntaxElement) -> Option<SyntaxToken> {
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => {
                match token.kind() {
                    SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT => continue,
                    SyntaxKind::COMMA => return Some(token),
                    _ => break,
                }
            }
            NodeOrToken::Node(_) => break,
        }
    }
    None
}

fn get_trailing_comment(mut element: SyntaxElement) -> Option<SyntaxToken> {
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => {
                match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::COMMENT => return Some(token),
                    _ => break,
                }
            }
            NodeOrToken::Node(_) => break,
        }
    }
    None
}

fn get_comments_on_next_lines(mut element: SyntaxElement) -> Vec<SyntaxToken> {
    let mut found_new_line = false;
    let mut comments = Vec::new();
    while let Some(sibling) = element.next_sibling_or_token() {
        element = sibling.clone();
        match sibling {
            NodeOrToken::Token(token) => {
                match token.kind() {
                    SyntaxKind::WHITESPACE => continue,
                    SyntaxKind::NEWLINE => found_new_line = true,
                    SyntaxKind::COMMENT => {
                        if found_new_line {
                            comments.push(token)
                        }
                    },
                    _ => break,
                }
            }
            NodeOrToken::Node(_) => break,
        }
    }
    comments
}

fn get_children_with_non_trivia_tokens(node: SyntaxNode) -> impl Iterator<Item=SyntaxElement> {
    node.children_with_tokens().filter_map(|c| match c {
        NodeOrToken::Token(token) => {
            if token.kind() != SyntaxKind::WHITESPACE && token.kind() != SyntaxKind::COMMENT {
                Some(token.into())
            } else {
                None
            }
        }
        NodeOrToken::Node(node) => Some(node.into()),
    })
}

fn get_child_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxNode, ()> {
    match node.children().find(|c| c.kind() == kind) {
        Some(node) => Ok(node),
        None => Err(()),
    }
}

fn get_token_with_kind(node: SyntaxNode, kind: SyntaxKind) -> Result<SyntaxToken, ()> {
    match node.children_with_tokens().find(|c| c.kind() == kind) {
        Some(NodeOrToken::Token(token)) => Ok(token),
        _ => Err(()),
    }
}

pub trait SyntaxElementExtensions {
    fn text(&self) -> String;
    fn child_comments(&self) -> Vec<SyntaxToken>;
    fn is_last_non_trivia_sibling(&self) -> bool;
}

impl SyntaxElementExtensions for SyntaxElement {
    fn text(&self) -> String {
        match self {
            NodeOrToken::Node(node) => node.text().to_string(),
            NodeOrToken::Token(token) => token.text().to_string(),
        }
    }

    fn child_comments(&self) -> Vec<SyntaxToken> {
        match self {
            NodeOrToken::Token(_) => Vec::with_capacity(0),
            NodeOrToken::Node(node) => node.children_with_tokens().filter_map(|c| match c {
                NodeOrToken::Token(token) if token.kind() == SyntaxKind::COMMENT => Some(token),
                _ => None,
            }).collect(),
        }
    }

    fn is_last_non_trivia_sibling(&self) -> bool {
        let mut element = self.clone();
        while let Some(sibling) = element.next_sibling_or_token() {
            element = sibling.clone();
            match sibling {
                NodeOrToken::Token(token) => {
                    match token.kind() {
                        SyntaxKind::WHITESPACE => continue,
                        SyntaxKind::NEWLINE => continue,
                        SyntaxKind::COMMENT => continue,
                        _ => return false,
                    }
                }
                NodeOrToken::Node(_) => return false,
            }
        }

        true
    }
}