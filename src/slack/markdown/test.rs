use serde_json::{json, Value};

use super::*;

fn render(markdown: &str) -> Value {
    serde_json::to_value(markdown_to_rich_text(markdown)).unwrap()
}

fn first_inline(markdown: &str) -> Value {
    render(markdown)["elements"][0]["elements"].clone()
}

#[test]
fn nested_markdown_styles_become_slack_text_styles() {
    assert_eq!(
        first_inline("**bold _and italic_** ~~gone~~"),
        json!([
            {"type": "text", "text": "bold ", "style": {"bold": true}},
            {"type": "text", "text": "and italic", "style": {"bold": true, "italic": true}},
            {"type": "text", "text": " "},
            {"type": "text", "text": "gone", "style": {"strike": true}}
        ])
    );
}

#[test]
fn headings_become_bold_sections() {
    let result = render("# Title\n\nParagraph");
    assert_eq!(
        result["elements"],
        json!([
            {"type": "rich_text_section", "elements": [{"type": "text", "text": "Title", "style": {"bold": true}}]},
            {"type": "rich_text_section", "elements": [{"type": "text", "text": "Paragraph"}]}
        ])
    );
    assert!(result.get("block_id").is_none());
}

#[test]
fn links_preserve_labels_urls_and_styles() {
    assert_eq!(
        first_inline("**[Label](https://example.com/?a=1&b=2)** and <https://example.org>"),
        json!([
            {"type": "link", "url": "https://example.com/?a=1&b=2", "text": "Label", "style": {"bold": true}},
            {"type": "text", "text": " and "},
            {"type": "link", "url": "https://example.org", "text": "https://example.org"}
        ])
    );
}

#[test]
fn inline_code_keeps_markdown_characters_literal() {
    assert_eq!(
        first_inline("`**literal** <@U123> &`"),
        json!([
            {"type": "text", "text": "**literal** <@U123> &", "style": {"code": true}}
        ])
    );
}

#[test]
fn escaped_markdown_and_entities_are_plain_text() {
    assert_eq!(
        first_inline(r"\*\*literal\*\* &amp; &lt;@U123&gt;"),
        json!([
            {"type": "text", "text": "**literal** & <@U123>"}
        ])
    );
}

#[test]
fn fenced_code_keeps_indentation_and_ignores_language_marker() {
    let result = render("```rust\nfn main() {\n    // **literal**\n}\n```");
    assert_eq!(
        result["elements"],
        json!([{
            "type": "rich_text_preformatted", "elements": [{
                "type": "text", "text": "fn main() {\n    // **literal**\n}\n"
            }]
        }])
    );
}

#[test]
fn nested_lists_keep_order_and_indent() {
    let result = render("- first\n  - nested\n- second");
    assert_eq!(
        result["elements"],
        json!([
            {"type": "rich_text_list", "style": "bullet", "indent": 0, "elements": [
                {"type": "rich_text_section", "elements": [{"type": "text", "text": "first"}]}
            ]},
            {"type": "rich_text_list", "style": "bullet", "indent": 1, "elements": [
                {"type": "rich_text_section", "elements": [{"type": "text", "text": "nested"}]}
            ]},
            {"type": "rich_text_list", "style": "bullet", "indent": 0, "elements": [
                {"type": "rich_text_section", "elements": [{"type": "text", "text": "second"}]}
            ]}
        ])
    );
}

#[test]
fn ordered_lists_keep_start_and_numbering_after_nested_list() {
    let result = render("5. first\n   - nested\n6. second");
    assert_eq!(result["elements"][0]["style"], "ordered");
    assert_eq!(result["elements"][0]["offset"], 4);
    assert_eq!(result["elements"][1]["indent"], 1);
    assert_eq!(result["elements"][2]["offset"], 5);
}

#[test]
fn quotes_preserve_styling_and_paragraphs() {
    let result = render("> quote **bold**\n>\n> second");
    assert_eq!(
        result["elements"],
        json!([{
            "type": "rich_text_quote", "elements": [
                {"type": "text", "text": "quote "},
                {"type": "text", "text": "bold", "style": {"bold": true}},
                {"type": "text", "text": "\n\nsecond"}
            ]
        }])
    );
}

#[test]
fn tables_become_readable_preformatted_text() {
    let result = render("| Name | Count |\n| --- | --- |\n| cat | 2 |");
    assert_eq!(result["elements"][0]["type"], "rich_text_preformatted");
    assert_eq!(
        result["elements"][0]["elements"][0]["text"],
        "Name | Count\n-----+------\ncat  | 2    "
    );
}

#[test]
fn unsupported_link_targets_remain_visible_as_text() {
    assert_eq!(
        first_inline("[Section](#section) [bad](javascript:alert)"),
        json!([
            {"type": "text", "text": "Section (#section) bad (javascript:alert)"}
        ])
    );
}

#[test]
fn streaming_partial_markdown_can_be_rendered_repeatedly() {
    assert_eq!(first_inline("**bo")[0]["text"], "**bo");
    assert_eq!(first_inline("**bold**")[0]["style"]["bold"], true);
    let result = render("```python\nprint('**literal**')");
    assert_eq!(result["elements"][0]["type"], "rich_text_preformatted");
    assert_eq!(
        result["elements"][0]["elements"][0]["text"],
        "print('**literal**')"
    );
}

#[test]
fn rich_text_round_trips_without_a_block_id() {
    let original = render("**Bold**\n\n- item\n\n> quote");
    let parsed: BlockElement = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), original);
}

#[test]
fn empty_response_has_a_nonempty_rich_text_section() {
    assert_eq!(first_inline(""), json!([{"type": "text", "text": " "}]));
}

#[test]
fn incomplete_empty_blocks_keep_valid_inline_elements() {
    fn assert_nonempty(element: &Value) {
        if let Some(elements) = element.get("elements") {
            let elements = elements.as_array().unwrap();
            assert!(!elements.is_empty());
            for element in elements {
                assert_nonempty(element);
            }
        }
        if element["type"] == "text" {
            assert!(!element["text"].as_str().unwrap().is_empty());
        }
    }
    for markdown in ["#", ">", "-", "```rust\n"] {
        assert_nonempty(&render(markdown));
    }
}

#[test]
fn task_lists_keep_their_checkboxes() {
    let result = render("- [ ] todo\n- [x] done");
    assert_eq!(result["elements"][0]["style"], "bullet");
    assert_eq!(
        result["elements"][0]["elements"][0]["elements"][0]["text"],
        "☐ todo"
    );
    assert_eq!(
        result["elements"][0]["elements"][1]["elements"][0]["text"],
        "☑ done"
    );
}

#[test]
fn linked_image_keeps_its_alt_text() {
    assert_eq!(
        first_inline("[![Diagram](https://example.com/image.png)](https://example.com/page)"),
        json!([
            {"type": "link", "url": "https://example.com/page", "text": "Diagram"}
        ])
    );
}

#[test]
fn bare_urls_become_links_without_surrounding_punctuation() {
    assert_eq!(
        first_inline("See https://example.com/hello?world=yes for details."),
        json!([
            {"type": "text", "text": "See "},
            {"type": "link", "url": "https://example.com/hello?world=yes", "text": "https://example.com/hello?world=yes"},
            {"type": "text", "text": " for details."}
        ])
    );
    assert_eq!(
        first_inline("**https://example.com.**"),
        json!([
            {"type": "link", "url": "https://example.com", "text": "https://example.com", "style": {"bold": true}},
            {"type": "text", "text": ".", "style": {"bold": true}}
        ])
    );
    assert_eq!(
        first_inline("`https://example.com`"),
        json!([
            {"type": "text", "text": "https://example.com", "style": {"code": true}}
        ])
    );
}

#[test]
fn quoted_blocks_keep_paragraph_boundaries() {
    let result = render("> intro\n>\n> - one\n> - two");
    assert_eq!(
        result["elements"][0]["elements"],
        json!([
            {"type": "text", "text": "intro\n\n• one\n• two"}
        ])
    );
    let result = render("> before\n>\n> ```rust\n> code\n> ```");
    assert_eq!(
        result["elements"][0]["elements"],
        json!([
            {"type": "text", "text": "before\n\n"},
            {"type": "text", "text": "code\n", "style": {"code": true}}
        ])
    );
    let result = render("> paragraph\n>\n> # heading");
    assert_eq!(
        result["elements"][0]["elements"],
        json!([
            {"type": "text", "text": "paragraph\n\n"},
            {"type": "text", "text": "heading", "style": {"bold": true}}
        ])
    );
}

#[test]
fn quoted_tables_keep_rows_and_columns() {
    let result = render("> | Name | Count |\n> | --- | --- |\n> | cat | 2 |");
    assert_eq!(result["elements"][0]["type"], "rich_text_quote");
    assert_eq!(
        result["elements"][0]["elements"],
        json!([
            {"type": "text", "text": "Name | Count\n-----+------\ncat  | 2    ", "style": {"code": true}}
        ])
    );
}

#[test]
fn bare_email_addresses_become_mailto_links() {
    assert_eq!(
        first_inline("Contact name@example.com."),
        json!([
            {"type": "text", "text": "Contact "},
            {"type": "link", "url": "mailto:name@example.com", "text": "name@example.com"},
            {"type": "text", "text": "."}
        ])
    );
}

#[test]
fn bare_urls_in_fenced_code_stay_literal() {
    let result = render("```text\nhttps://example.com\n```");
    assert_eq!(
        result["elements"][0]["elements"],
        json!([
            {"type": "text", "text": "https://example.com\n"}
        ])
    );
}
