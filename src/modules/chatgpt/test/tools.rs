use super::*;

#[test]
fn strict_tool_schema_only_allows_null_for_optional_parameters() -> anyhow::Result<()> {
    let arguments = HashMap::from([
        (
            "time".to_string(),
            ("string".to_string(), "Time to convert".to_string()),
        ),
        (
            "timezone".to_string(),
            ("string".to_string(), "IANA timezone".to_string()),
        ),
        (
            "days_ago".to_string(),
            ("integer".to_string(), "Days before now".to_string()),
        ),
    ]);
    let required = HashSet::from(["time".to_string()]);
    let parameters = serde_json::to_value(FunctionCallParameters::new(arguments, &required))?;

    assert_eq!(parameters["properties"]["time"]["type"], json!(["string"]));
    assert_eq!(
        parameters["properties"]["timezone"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(
        parameters["properties"]["days_ago"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(parameters["additionalProperties"], false);
    let strict_required: HashSet<String> = serde_json::from_value(parameters["required"].clone())?;
    assert_eq!(
        strict_required,
        HashSet::from([
            "time".to_string(),
            "timezone".to_string(),
            "days_ago".to_string(),
        ])
    );
    Ok(())
}

#[test]
fn all_optional_mcp_parameters_are_nullable_in_strict_schema() -> anyhow::Result<()> {
    let tool = serde_json::from_str::<rmcp::model::Tool>(
        r#"{"name":"get_current_time","description":"Get current time","inputSchema":{"type":"object","properties":{"timezone":{"type":"string","default":"Asia/Seoul"}}}}"#,
    )?;
    let (_, arguments, required) = crate::DittoBot::tool_metadata("mcp-server-time", &tool)?;
    let parameters = serde_json::to_value(FunctionCallParameters::new(arguments, &required))?;

    assert_eq!(
        parameters["properties"]["timezone"]["type"],
        json!(["string", "null"])
    );
    assert_eq!(parameters["required"], json!(["timezone"]));
    Ok(())
}

#[test]
fn null_arguments_are_omitted_so_mcp_defaults_apply() -> anyhow::Result<()> {
    let arguments = ResponsesToolOutput::parse_arguments(
        r#"{"timezone":null,"days_ago":0,"enabled":false,"text":"","items":[]}"#,
    )?;
    assert!(!arguments.contains_key("timezone"));
    assert_eq!(arguments.len(), 4);
    assert_eq!(arguments.get("days_ago"), Some(&json!(0)));
    assert_eq!(arguments.get("enabled"), Some(&json!(false)));
    assert_eq!(arguments.get("text"), Some(&json!("")));
    assert_eq!(arguments.get("items"), Some(&json!([])));
    Ok(())
}

#[test]
fn malformed_tool_arguments_are_reported_instead_of_using_empty_arguments() {
    for arguments in ["not json", "[]", "null"] {
        let error = ResponsesToolOutput::parse_arguments(arguments).unwrap_err();
        assert!(error
            .to_string()
            .contains("OpenAI returned invalid tool arguments"));
    }
}
