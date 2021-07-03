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
    let mut context = Context {
        text,
        config,
        handled_comments: HashSet::new(),
    };

    let mut items = parse_node(node, &mut context);
    items.push_condition(if_true(
        "endOfFileNewLine",
        |context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0),
        Signal::NewLine.into()
    ));
    items
}

fn parse_node<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItems {
    parse_node_with_inner(node, context, |items, _| items)
}

fn parse_node_with_inner<'a>(
    node: SyntaxNode,
    context: &mut Context<'a>,
    inner_parse: impl FnOnce(PrintItems, &mut Context<'a>) -> PrintItems,
) -> PrintItems {
    let mut items = PrintItems::new();
    println!("{:?}", node);

    let result = match node.kind() {
        SyntaxKind::ROOT => parse_root(node.clone(), context),
        SyntaxKind::ARRAY => parse_array(node.clone(), context),
        SyntaxKind::ENTRY => parse_entry(node.clone(), context),
        SyntaxKind::KEY => parse_key(node.clone(), context),
        SyntaxKind::VALUE => parse_value(node.clone(), context),
        _ => Err(()),
    };

    items.extend(inner_parse(match result {
        Ok(items) => items,
        Err(()) => parse_raw_string_trim_line_ends(node.text().to_string().trim().into()),
    }, context));

    items
}

fn parse_root<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    print_formatted_tree(node.clone());

    let mut items = PrintItems::new();
    for (i, child) in node.children().enumerate() {
        if i > 0 {
            items.push_signal(Signal::NewLine);
        }
        items.extend(parse_node(child, context));
    }

    Ok(items)
}

fn parse_array<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let values = node.children();
    ensure_all_kind(values.clone(), SyntaxKind::VALUE)?;

    let mut items = PrintItems::new();

    // todo: parse_surrounded_by_nodes
    items.push_str("[");
    items.extend(parse_comma_separated_values(ParseCommaSeparatedValuesOptions {
        nodes: values.collect::<Vec<_>>(),
        prefer_hanging: false,
        force_use_new_lines: false,
        allow_blank_lines: true,
        single_line_space_at_start: false,
        single_line_space_at_end: false,
        custom_single_line_separator: None,
        multi_line_options: MultiLineOptions::surround_newlines_indented(),
        force_possible_newline_at_start: false,
    }, context));
    items.push_str("]");

    Ok(items)
}

fn parse_entry<'a>(node: SyntaxNode, context: &mut Context<'a>) -> PrintItemsResult {
    let key = get_child_with_kind(node.clone(), SyntaxKind::KEY)?;
    let value = get_child_with_kind(node.clone(), SyntaxKind::VALUE)?;
    let mut items = PrintItems::new();

    items.extend(parse_node(key, context));
    items.push_str(" = ");
    items.extend(parse_node(value, context));

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
        items.extend(match element {
            NodeOrToken::Node(node) => parse_node(node, context),
            NodeOrToken::Token(token) => parse_token(token, context),
        });
    }
    items
}

fn parse_token<'a>(token: SyntaxToken, _: &mut Context<'a>) -> PrintItems {
    token.text().as_str().into()
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
    items.extend(parse_node_with_inner(value, context, move |mut items, _| {
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

fn parse_comment<'a>(comment: SyntaxToken, _context: &mut Context<'a>) -> PrintItems {
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
        println!("{:?}", sibling);
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

fn get_non_trivia_tokens(node: SyntaxNode) -> impl Iterator<Item=SyntaxToken> {
    get_children_with_non_trivia_tokens(node).filter_map(|c| match c {
        NodeOrToken::Token(token) => Some(token),
        NodeOrToken::Node(_) => None,
    })
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
