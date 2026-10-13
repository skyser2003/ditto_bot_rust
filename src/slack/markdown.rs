use linkify::{LinkFinder, LinkKind};
use pulldown_cmark::{Event, Options, Parser, Tag};

use super::{BlockElement, LinkBlock, RichTextListStyle, RichTextStyle};

#[cfg(test)]
mod test;

enum Node<'a> {
    Element(Tag<'a>, Vec<Node<'a>>),
    Leaf(Event<'a>),
}

/// Render CommonMark as Slack's native rich_text elements, without changing
/// the original response used for streaming accumulation and thread history.
pub fn markdown_to_rich_text(markdown: &str) -> BlockElement {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let nodes = parse_nodes(&mut Parser::new_ext(markdown, options));
    let mut elements = render_blocks(&nodes, 0);
    if elements.is_empty() {
        elements.push(section(vec![text_element(" ", RichTextStyle::default())]));
    }
    BlockElement::RichText {
        block_id: None,
        elements: linkify_elements(elements, &LinkFinder::new()),
    }
}

fn parse_nodes<'a>(events: &mut impl Iterator<Item = Event<'a>>) -> Vec<Node<'a>> {
    let mut nodes = vec![];
    while let Some(event) = events.next() {
        match event {
            Event::Start(tag) => nodes.push(Node::Element(tag, parse_nodes(events))),
            Event::End(_) => break,
            event => nodes.push(Node::Leaf(event)),
        }
    }
    nodes
}

fn section(elements: Vec<BlockElement>) -> BlockElement {
    BlockElement::RichTextSection {
        elements: nonempty_inline(elements),
    }
}

fn nonempty_inline(mut elements: Vec<BlockElement>) -> Vec<BlockElement> {
    if elements.is_empty() {
        elements.push(text_element(" ", RichTextStyle::default()));
    }
    elements
}

fn text_element(text: &str, style: RichTextStyle) -> BlockElement {
    BlockElement::Text {
        text: text.to_string(),
        style: style.as_option(),
    }
}

fn push_text(elements: &mut Vec<BlockElement>, text: &str, style: RichTextStyle) {
    if text.is_empty() {
        return;
    }
    let style = style.as_option();
    if let Some(BlockElement::Text {
        text: previous,
        style: previous_style,
    }) = elements.last_mut()
    {
        if *previous_style == style {
            previous.push_str(text);
            return;
        }
    }
    elements.push(BlockElement::Text {
        text: text.to_string(),
        style,
    });
}

fn extend_inline(elements: &mut Vec<BlockElement>, additional: Vec<BlockElement>) {
    for element in additional {
        match element {
            BlockElement::Text { text, style } => {
                push_text(elements, &text, style.unwrap_or_default())
            }
            element => elements.push(element),
        }
    }
}

fn valid_url(url: &str) -> bool {
    url::Url::parse(url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https" | "mailto" | "tel" | "slack"))
}

fn linkify_elements(elements: Vec<BlockElement>, finder: &LinkFinder) -> Vec<BlockElement> {
    let mut linked = vec![];
    for element in elements {
        match element {
            BlockElement::Text { text, style } if !style.unwrap_or_default().code => {
                let mut end = 0;
                for link in finder.links(&text) {
                    let url = match link.kind() {
                        LinkKind::Email => format!("mailto:{}", link.as_str()),
                        _ => link.as_str().to_string(),
                    };
                    if !valid_url(&url) {
                        continue;
                    }
                    push_text(
                        &mut linked,
                        &text[end..link.start()],
                        style.unwrap_or_default(),
                    );
                    linked.push(BlockElement::Link(LinkBlock {
                        url,
                        text: Some(link.as_str().to_string()),
                        style,
                    }));
                    end = link.end();
                }
                push_text(&mut linked, &text[end..], style.unwrap_or_default());
            }
            BlockElement::RichTextSection { elements } => {
                linked.push(section(linkify_elements(elements, finder)))
            }
            BlockElement::RichTextQuote { elements } => linked.push(BlockElement::RichTextQuote {
                elements: linkify_elements(elements, finder),
            }),
            BlockElement::RichTextList {
                style,
                indent,
                offset,
                elements,
            } => linked.push(BlockElement::RichTextList {
                style,
                indent,
                offset,
                elements: linkify_elements(elements, finder),
            }),
            element => linked.push(element),
        }
    }
    linked
}

fn render_inline(nodes: &[Node<'_>], style: RichTextStyle) -> Vec<BlockElement> {
    let mut elements = vec![];
    for node in nodes {
        match node {
            Node::Leaf(Event::Text(text) | Event::Html(text) | Event::InlineHtml(text)) => {
                push_text(&mut elements, text, style)
            }
            Node::Leaf(Event::Code(text)) => push_text(
                &mut elements,
                text,
                RichTextStyle {
                    code: true,
                    ..style
                },
            ),
            Node::Leaf(Event::SoftBreak | Event::HardBreak) => {
                push_text(&mut elements, "\n", style)
            }
            Node::Leaf(Event::TaskListMarker(checked)) => {
                push_text(&mut elements, if *checked { "☑ " } else { "☐ " }, style)
            }
            Node::Leaf(Event::Rule) => push_text(&mut elements, "────────", style),
            Node::Element(tag, children) => {
                let mut child_style = style;
                if !elements.is_empty()
                    && matches!(
                        tag,
                        Tag::Paragraph
                            | Tag::Heading { .. }
                            | Tag::CodeBlock(_)
                            | Tag::List(_)
                            | Tag::Table(_)
                            | Tag::BlockQuote(_)
                    )
                {
                    push_text(&mut elements, "\n\n", style);
                }
                match tag {
                    Tag::Strong | Tag::Heading { .. } => child_style.bold = true,
                    Tag::Emphasis => child_style.italic = true,
                    Tag::Strikethrough => child_style.strike = true,
                    Tag::CodeBlock(_) => child_style.code = true,
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        let mut label = render_inline(children, style);
                        if label.is_empty() {
                            label.push(text_element(dest_url, style));
                        }
                        if valid_url(dest_url) {
                            for element in label {
                                let (text, style) = match element {
                                    BlockElement::Text { text, style } => (text, style),
                                    BlockElement::Link(link) => {
                                        (link.text.unwrap_or(link.url), link.style)
                                    }
                                    _ => continue,
                                };
                                elements.push(BlockElement::Link(LinkBlock {
                                    url: dest_url.to_string(),
                                    text: Some(text),
                                    style,
                                }));
                            }
                        } else {
                            extend_inline(&mut elements, label);
                            push_text(&mut elements, &format!(" ({})", dest_url), style);
                        }
                        continue;
                    }
                    Tag::List(start) => {
                        for (index, item) in children.iter().enumerate() {
                            if index > 0 {
                                push_text(&mut elements, "\n", style);
                            }
                            let prefix = start
                                .map(|start| format!("{}. ", start + index as u64))
                                .unwrap_or_else(|| "• ".to_string());
                            push_text(&mut elements, &prefix, style);
                            extend_inline(
                                &mut elements,
                                render_inline(std::slice::from_ref(item), style),
                            );
                        }
                        continue;
                    }
                    Tag::Table(_) => {
                        if let BlockElement::RichTextPreformatted { elements: table } =
                            render_table(children)
                        {
                            for element in table {
                                if let BlockElement::Text { text, .. } = element {
                                    push_text(
                                        &mut elements,
                                        &text,
                                        RichTextStyle {
                                            code: true,
                                            ..style
                                        },
                                    );
                                }
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
                extend_inline(&mut elements, render_inline(children, child_style));
            }
            _ => {}
        }
    }
    elements
}

fn render_blocks(nodes: &[Node<'_>], indent: u32) -> Vec<BlockElement> {
    let mut blocks = vec![];
    let mut pending = vec![];
    for node in nodes {
        let block = match node {
            Node::Element(Tag::Paragraph, children) => {
                Some(section(render_inline(children, RichTextStyle::default())))
            }
            Node::Element(Tag::Heading { .. }, children) => Some(section(render_inline(
                children,
                RichTextStyle {
                    bold: true,
                    ..Default::default()
                },
            ))),
            Node::Element(Tag::CodeBlock(_), children) => {
                let text = plain_text(children);
                Some(BlockElement::RichTextPreformatted {
                    elements: vec![text_element(
                        if text.is_empty() { " " } else { &text },
                        RichTextStyle::default(),
                    )],
                })
            }
            Node::Element(Tag::BlockQuote(_), children) => Some(BlockElement::RichTextQuote {
                elements: nonempty_inline(render_inline(children, RichTextStyle::default())),
            }),
            Node::Element(Tag::Table(_), children) => Some(render_table(children)),
            Node::Element(Tag::List(start), children) => {
                if !pending.is_empty() {
                    blocks.push(section(std::mem::take(&mut pending)));
                }
                blocks.extend(render_list(children, *start, indent));
                continue;
            }
            Node::Leaf(Event::Rule) => Some(section(vec![text_element(
                "────────",
                RichTextStyle::default(),
            )])),
            _ => None,
        };
        if let Some(block) = block {
            if !pending.is_empty() {
                blocks.push(section(std::mem::take(&mut pending)));
            }
            blocks.push(block);
        } else {
            pending.extend(render_inline(
                std::slice::from_ref(node),
                RichTextStyle::default(),
            ));
        }
    }
    if !pending.is_empty() {
        blocks.push(section(pending));
    }
    blocks
}

fn flush_list(
    blocks: &mut Vec<BlockElement>,
    items: &mut Vec<BlockElement>,
    start: Option<u64>,
    indent: u32,
    offset: &mut u64,
) {
    if items.is_empty() {
        return;
    }
    let next_offset = *offset + items.len() as u64;
    blocks.push(BlockElement::RichTextList {
        style: if start.is_some() {
            RichTextListStyle::Ordered
        } else {
            RichTextListStyle::Bullet
        },
        indent,
        offset: (start.is_some() && *offset > 0).then_some(*offset),
        elements: std::mem::take(items),
    });
    *offset = next_offset;
}

fn render_list(nodes: &[Node<'_>], start: Option<u64>, indent: u32) -> Vec<BlockElement> {
    let mut blocks = vec![];
    let mut items = vec![];
    let mut offset = start.unwrap_or(1).saturating_sub(1);
    for node in nodes {
        if let Node::Element(Tag::Item, children) = node {
            let mut item_blocks = render_blocks(children, indent + 1).into_iter().peekable();
            let mut content = match item_blocks.peek() {
                Some(BlockElement::RichTextSection { .. }) => match item_blocks.next().unwrap() {
                    BlockElement::RichTextSection { elements } => elements,
                    _ => unreachable!(),
                },
                _ => vec![text_element(" ", RichTextStyle::default())],
            };
            while matches!(
                item_blocks.peek(),
                Some(BlockElement::RichTextSection { .. })
            ) {
                if let Some(BlockElement::RichTextSection { elements }) = item_blocks.next() {
                    content.push(text_element("\n\n", RichTextStyle::default()));
                    content.extend(elements);
                }
            }
            items.push(section(content));
            if item_blocks.peek().is_some() {
                flush_list(&mut blocks, &mut items, start, indent, &mut offset);
                blocks.extend(item_blocks);
            }
        }
    }
    flush_list(&mut blocks, &mut items, start, indent, &mut offset);
    blocks
}

fn plain_text(nodes: &[Node<'_>]) -> String {
    render_inline(nodes, RichTextStyle::default())
        .into_iter()
        .filter_map(|element| match element {
            BlockElement::Text { text, .. } => Some(text),
            BlockElement::Link(link) => Some(link.text.unwrap_or(link.url)),
            _ => None,
        })
        .collect()
}

fn render_table(nodes: &[Node<'_>]) -> BlockElement {
    let rows: Vec<Vec<String>> = nodes
        .iter()
        .filter_map(|node| match node {
            Node::Element(Tag::TableHead | Tag::TableRow, cells) => Some(
                cells
                    .iter()
                    .map(|cell| match cell {
                        Node::Element(Tag::TableCell, children) => {
                            plain_text(children).replace('\n', " ")
                        }
                        _ => String::new(),
                    })
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or_default();
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let mut lines = vec![];
    for (index, row) in rows.iter().enumerate() {
        lines.push(
            row.iter()
                .enumerate()
                .map(|(column, cell)| {
                    format!(
                        "{}{}",
                        cell,
                        " ".repeat(widths[column] - cell.chars().count())
                    )
                })
                .collect::<Vec<_>>()
                .join(" | "),
        );
        if index == 0 {
            lines.push(
                widths
                    .iter()
                    .map(|width| "-".repeat(*width))
                    .collect::<Vec<_>>()
                    .join("-+-"),
            );
        }
    }
    BlockElement::RichTextPreformatted {
        elements: vec![text_element(&lines.join("\n"), RichTextStyle::default())],
    }
}
