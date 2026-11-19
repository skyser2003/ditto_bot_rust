use std::collections::HashSet;

use crate::DittoBot;

struct ToolFixture;

impl ToolFixture {
    fn parse(schema: &str) -> rmcp::model::Tool {
        let input_schema: rmcp::model::JsonObject =
            serde_json::from_str(schema).expect("Fixture schema should be valid JSON");
        rmcp::model::Tool::new("get_current_time", "Test tool", input_schema)
    }
}

#[test]
fn mcp_tool_without_required_keeps_optional_parameters() -> anyhow::Result<()> {
    let tool = ToolFixture::parse(
        r#"{"type":"object","properties":{"timezone":{"type":"string","default":"Asia/Seoul","description":"IANA timezone"}}}"#,
    );
    let (name, arguments, required) = DittoBot::tool_metadata("mcp-server-time", &tool)?;

    assert_eq!(name, "mcp-server-time_get_current_time");
    assert_eq!(arguments.len(), 1);
    assert_eq!(
        arguments.get("timezone"),
        Some(&("string".to_string(), "IANA timezone".to_string()))
    );
    assert!(required.is_empty());
    Ok(())
}

#[test]
fn mcp_tool_preserves_explicit_required_parameters() -> anyhow::Result<()> {
    let tool = ToolFixture::parse(
        r#"{"type":"object","properties":{"time":{"type":"string"},"timezone":{"type":"string"}},"required":["time"]}"#,
    );
    let (_, arguments, required) = DittoBot::tool_metadata("mcp-server-time", &tool)?;

    assert_eq!(arguments.len(), 2);
    assert_eq!(required, HashSet::from(["time".to_string()]));
    Ok(())
}

#[test]
fn mcp_tools_accept_empty_required_and_parameterless_schemas() -> anyhow::Result<()> {
    for schema in [
        r#"{"type":"object"}"#,
        r#"{"type":"object","properties":{}}"#,
        r#"{"type":"object","properties":{},"required":[]}"#,
    ] {
        let tool = ToolFixture::parse(schema);
        let (_, arguments, required) = DittoBot::tool_metadata("test", &tool)?;
        assert!(arguments.is_empty());
        assert!(required.is_empty());
    }
    Ok(())
}

#[test]
fn malformed_mcp_schemas_return_tool_context_instead_of_panicking() {
    for (schema, expected) in [
        (r#"{"properties":null}"#, "properties must be an object"),
        (r#"{"properties":[]}"#, "properties must be an object"),
        (
            r#"{"required":null}"#,
            "required must be an array of strings",
        ),
        (
            r#"{"required":"timezone"}"#,
            "required must be an array of strings",
        ),
        (
            r#"{"required":[123]}"#,
            "required must be an array of strings",
        ),
        (
            r#"{"properties":{"timezone":null}}"#,
            "parameter timezone must be an object",
        ),
    ] {
        let tool = ToolFixture::parse(schema);
        let error = DittoBot::tool_metadata("mcp-server-time", &tool).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("mcp-server-time_get_current_time"));
        assert!(message.contains(expected), "{}", message);
    }
}
