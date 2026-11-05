use std::{borrow::Cow, collections::HashMap, env, future::Future, panic::AssertUnwindSafe};

use anyhow::{anyhow, bail, ensure, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::{FutureExt, StreamExt};
use log::{debug, error, warn};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, PostMessageResponse, SectionBlock, SlackFile, ThreadMessageType},
    Bot, Message, ReplyMessageEvent,
};

mod moderation;

const PROCESSING_REACTION: &str = "hourglass_flowing_sand";
const ERROR_MESSAGE: &str = "An error occurred while processing your request. Please try again.";

#[cfg(test)]
mod test;

#[derive(Debug, Serialize)]
#[serde(untagged)]
#[serde(rename_all = "snake_case")]
enum ResponsesInput {
    Message(OpenAIResponsesMessage),
    FunctionCall(ResponsesToolOutput),
}

#[derive(Debug, Serialize)]
struct OpenAIResponsesMessage {
    role: String,
    content: ResponsesMessageContent,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum ResponsesMessageContent {
    Text(String),
    Parts(Vec<ResponsesContentPart>),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResponsesContentPart {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        detail: &'static str,
    },
}

#[derive(Debug, Serialize)]
struct OpenAIResponsesBody {
    model: String,
    input: Vec<ResponsesInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    previous_response_id: Option<String>,
    store: bool,
    stream: bool,
    tools: Vec<OpenAIResponsesTool>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStreamingResponse {
    #[allow(dead_code)]
    #[serde(rename = "response.output_text.delta")]
    Delta { item_id: String, delta: String },
    #[serde(rename = "response.completed")]
    Completed {
        response: ResponsesCompletedResponse,
    },
    #[serde(rename = "response.failed")]
    Failed,
    #[serde(rename = "response.incomplete")]
    Incomplete,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "response.created")]
    Created,
    #[serde(rename = "response.in_progress")]
    InProgress,
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded,
    #[serde(rename = "response.output_item.done")]
    OutputItemDone,
    #[serde(rename = "response.output_text.done")]
    OutputTextDone,
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded,
    #[serde(rename = "response.content_part.done")]
    ContentPartDone,
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta,
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResponsesCompletedResponse {
    #[allow(dead_code)]
    id: String,
    status: String,
    // If empty, skip
    #[serde(default)]
    output: Vec<ResponsesStreamingOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStreamingOutput {
    Reasoning {
        #[allow(dead_code)]
        id: String,
    },
    #[allow(dead_code)]
    Message {
        id: String,
        status: String,
        role: String,
        content: Vec<ResponsesStreamingContent>,
    },
    FunctionCall {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        status: String,
        arguments: String,
        call_id: String,
        name: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesToolOutput {
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ResponsesStreamingContent {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    type_field: String,
    text: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
enum OpenAIResponsesTool {
    Function(FunctionCallBody),
    #[serde(rename = "web_search_preview")]
    WebSearch,
    CodeInterpreter(CodeInterpreterBody),
    Mcp(McpBody),
}

#[derive(Debug, Serialize)]
struct FunctionCallBody {
    name: String,
    description: String,
    parameters: FunctionCallParameters,
    strict: bool,
}

#[derive(Debug, Serialize)]
struct FunctionCallParameters {
    #[serde(rename = "type")]
    type_field: String,
    properties: HashMap<String, FunctionCallParameter>,
    required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    additional_properties: bool,
}

#[derive(Debug, Serialize)]
struct FunctionCallParameter {
    #[serde(rename = "type")]
    type_field: Vec<String>,
    description: String,
}

#[derive(Debug, Serialize)]
struct CodeInterpreterBody {
    pub container: CodeInterpreterContainerType,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CodeInterpreterContainerType {
    Auto {
        r#type: &'static str,
    },
    #[allow(dead_code)]
    ContainerId(String),
}

#[derive(Debug, Serialize)]
struct McpBody {
    server_label: String,
    server_url: String,
    require_approval: String,
}

pub async fn handle<B: Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    let slack_bot_format = format!("<@{}>", bot.bot_id());
    let is_bot_command = msg.text.contains(&slack_bot_format);

    if !is_bot_command {
        return Ok(());
    }

    if let Some(input) = moderation::parse_command(&msg.text, &slack_bot_format) {
        return moderation::handle(bot, msg, input).await;
    }

    let command_str = msg.text.replace(&slack_bot_format, "");

    if command_str.trim().is_empty() && !msg.files.iter().any(SlackFile::is_image) {
        return Ok(());
    }

    process_request(bot, msg, respond(bot, msg, &slack_bot_format, &command_str)).await
}

async fn process_request<B: Bot>(
    bot: &B,
    msg: &crate::MessageEvent,
    response: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    if let Err(error) = bot
        .add_reaction(&msg.channel, &msg.ts, PROCESSING_REACTION)
        .await
    {
        warn!("Failed to add ChatGPT processing reaction: {:#}", error);
    }

    let result = AssertUnwindSafe(response)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| Err(anyhow!("ChatGPT request panicked")));

    if let Err(error) = &result {
        error!("ChatGPT request failed: {:#}", error);
        let reply = Some(ReplyMessageEvent {
            msg: msg.thread_ts.clone().unwrap_or_else(|| msg.ts.clone()),
            broadcast: msg.reply_broadcast,
        });
        match bot
            .send_message(&msg.channel, Message::Text(ERROR_MESSAGE), reply, None)
            .await
        {
            Ok(sent) if sent.ok => {}
            Ok(sent) => error!(
                "Slack rejected ChatGPT error notification: {:?}",
                sent.error
            ),
            Err(error) => error!("Failed to send ChatGPT error notification: {:#}", error),
        }
    }

    // Final cleanup runs after both the response and any error notification.
    if let Err(error) = bot
        .remove_reaction(&msg.channel, &msg.ts, PROCESSING_REACTION)
        .await
    {
        warn!("Failed to remove ChatGPT processing reaction: {:#}", error);
    }

    result
}

async fn respond<B: Bot>(
    bot: &B,
    msg: &crate::MessageEvent,
    slack_bot_format: &str,
    command_str: &str,
) -> anyhow::Result<()> {
    let call_type = command_str.split_whitespace().next().unwrap_or("");

    let gpt_split = call_type.split("gpt").collect::<Vec<_>>();

    let gpt_prefix_exists = call_type.starts_with("gpt");

    let call_prefix = if !gpt_prefix_exists {
        format!("{} ", slack_bot_format)
    } else {
        format!("{} {} ", slack_bot_format, call_type)
    };

    debug!("GPT: bot command full text = {:?}", msg.text);

    let input_text = parse_input_text(command_str, gpt_prefix_exists);

    let openai_req = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
        .build()?;

    let thread_ts = if let Some(thread_ts) = msg.thread_ts.clone() {
        thread_ts
    } else {
        msg.ts.clone()
    };

    let conv_fut = bot.get_conversation_replies(&msg.channel, thread_ts.as_str());
    let conv_result = conv_fut.await;

    let stream_mode_str = env::var("USE_GPT_STREAM").unwrap_or("true".to_string());
    let stream_mode_str = stream_mode_str.to_lowercase();

    let stream_mode = stream_mode_str == "1" || stream_mode_str.to_lowercase() == "true";

    let openai_model = env::var("OPENAI_MODEL").unwrap_or("gpt-4o-mini".to_string());
    let temperature = if openai_model.starts_with("o") || openai_model.starts_with("gpt-5") {
        Some(1.0)
    } else if openai_model.starts_with("gpt-6") {
        None
    } else {
        gpt_split.get(1).and_then(|value| value.parse::<f32>().ok())
    };

    let mut tools = vec![];

    // Unused for now
    if false {
        tools.push(OpenAIResponsesTool::CodeInterpreter(CodeInterpreterBody {
            container: CodeInterpreterContainerType::Auto { r#type: "auto" },
        }));
    }

    if !openai_model.starts_with("o") {
        tools.push(OpenAIResponsesTool::WebSearch);
    }

    // Remote mcp tools
    if false {
        tools.push(OpenAIResponsesTool::Mcp(McpBody {
            server_label: "deepwiki".to_string(),
            server_url: "https://mcp.deepwiki.com/sse".to_string(),
            require_approval: "never".to_string(),
        }));
    }

    let all_tools = bot.get_all_tools_metadata().await?;

    for (unified_name, arguments, required) in all_tools {
        let tool_call_body = FunctionCallBody {
            name: unified_name.clone(),
            description: format!("Call tool {}", unified_name),
            parameters: FunctionCallParameters {
                type_field: "object".to_string(),
                required: arguments.keys().cloned().collect(),
                properties: arguments
                    .iter()
                    .map(|(arg_name, (arg_type, description))| {
                        let is_optional = required.contains(arg_name);

                        let type_field = if is_optional {
                            vec![arg_type.clone(), "null".to_string()]
                        } else {
                            vec![arg_type.clone()]
                        };

                        (
                            arg_name.clone(),
                            FunctionCallParameter {
                                type_field,
                                description: description.clone(),
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>(),
                additional_properties: false,
            },
            strict: true,
        };

        tools.push(OpenAIResponsesTool::Function(tool_call_body));
    }

    let mut openai_body = OpenAIResponsesBody {
        model: openai_model,
        input: vec![],
        temperature,
        stream: stream_mode,
        store: true,
        previous_response_id: None,
        tools,
    };

    let messages = conv_result
        .as_ref()
        .ok()
        .filter(|response| response.ok)
        .and_then(|response| response.messages.as_deref())
        .unwrap_or_default();
    let reply_event = Some(ReplyMessageEvent {
        msg: thread_ts,
        broadcast: msg.reply_broadcast,
    });

    openai_body.input =
        build_responses_input(bot, msg, &input_text, &call_prefix, messages).await?;

    deliver_response(
        bot,
        msg,
        &openai_req,
        openai_body,
        "https://api.openai.com/v1/responses",
        reply_event,
    )
    .await
}

async fn deliver_response<B: Bot>(
    bot: &B,
    msg: &crate::MessageEvent,
    client: &reqwest::Client,
    mut body: OpenAIResponsesBody,
    url: &str,
    reply_event: Option<ReplyMessageEvent>,
) -> anyhow::Result<()> {
    let request = client.post(url).bearer_auth(bot.openai_key()).json(&body);

    if body.stream {
        let mut gpt_message = GptMessageManager::new(&msg.channel, reply_event);
        let mut initial_received = false;
        let mut events = EventSource::new(request)?;

        while let Some(event) = events.next().await {
            match event.context("OpenAI response stream failed")? {
                Event::Open => debug!("OpenAI SSE opened"),
                Event::Message(event) => {
                    let event: ResponsesStreamingResponse = serde_json::from_str(&event.data)
                        .context("OpenAI returned an invalid stream event")?;
                    match event {
                        ResponsesStreamingResponse::Delta { delta, .. } => {
                            if !initial_received {
                                initial_received = true;
                                gpt_message
                                    .stream_message(bot, Some("`Receiving...`"))
                                    .await?;
                            }
                            gpt_message.concat_message(&delta);
                            if [",", ".", "?", "!", "\n"].contains(&delta.as_str()) {
                                gpt_message
                                    .stream_message(bot, Some(" `[continue]`"))
                                    .await?;
                            }
                        }
                        ResponsesStreamingResponse::Completed { response } => {
                            let (text, function_calls) = response_output(bot, &response).await?;
                            if function_calls.is_empty() {
                                if gpt_message.message.trim().is_empty() {
                                    gpt_message.message = text;
                                }
                                ensure!(
                                    !gpt_message.message.trim().is_empty(),
                                    "OpenAI returned no response text"
                                );
                                gpt_message.stream_message(bot, Some("`[DONE]`")).await?;
                                return Ok(());
                            }
                            body.previous_response_id = Some(response.id);
                            body.input.extend(function_calls);
                            events = EventSource::new(
                                client.post(url).bearer_auth(bot.openai_key()).json(&body),
                            )?;
                        }
                        ResponsesStreamingResponse::Failed
                        | ResponsesStreamingResponse::Incomplete
                        | ResponsesStreamingResponse::Error => {
                            bail!("OpenAI reported an unsuccessful response");
                        }
                        ResponsesStreamingResponse::Created
                        | ResponsesStreamingResponse::InProgress
                        | ResponsesStreamingResponse::OutputItemAdded
                        | ResponsesStreamingResponse::OutputItemDone
                        | ResponsesStreamingResponse::OutputTextDone
                        | ResponsesStreamingResponse::ContentPartAdded
                        | ResponsesStreamingResponse::ContentPartDone
                        | ResponsesStreamingResponse::FunctionCallArgumentsDelta
                        | ResponsesStreamingResponse::FunctionCallArgumentsDone
                        | ResponsesStreamingResponse::Unknown => {}
                    }
                }
            }
        }
        bail!("OpenAI stream ended before completing the response")
    } else {
        let mut request = request;
        loop {
            let response: ResponsesCompletedResponse = request
                .send()
                .await
                .context("OpenAI request failed")?
                .error_for_status()
                .context("OpenAI returned an unsuccessful HTTP status")?
                .json()
                .await
                .context("OpenAI returned an invalid response")?;
            let (text, function_calls) = response_output(bot, &response).await?;
            if function_calls.is_empty() {
                ensure!(!text.trim().is_empty(), "OpenAI returned no response text");
                return GptMessageManager::send_message_static(
                    bot,
                    &text,
                    &msg.channel,
                    &reply_event,
                )
                .await
                .map(|_| ());
            }
            body.previous_response_id = Some(response.id);
            body.input.extend(function_calls);
            request = client.post(url).bearer_auth(bot.openai_key()).json(&body);
        }
    }
}

async fn response_output<B: Bot>(
    bot: &B,
    response: &ResponsesCompletedResponse,
) -> anyhow::Result<(String, Vec<ResponsesInput>)> {
    ensure!(
        response.status == "completed",
        "OpenAI response did not complete"
    );
    let mut texts = vec![];
    let mut function_calls = vec![];
    for output in &response.output {
        match output {
            ResponsesStreamingOutput::Message {
                status, content, ..
            } => {
                ensure!(status == "completed", "OpenAI message did not complete");
                texts.extend(content.iter().map(|part| part.text.as_str()));
            }
            ResponsesStreamingOutput::FunctionCall {
                name,
                call_id,
                arguments,
                status,
                ..
            } => {
                ensure!(status == "completed", "OpenAI tool call did not complete");
                function_calls.push(get_function_call(bot, name, call_id, arguments).await?);
            }
            ResponsesStreamingOutput::Reasoning { .. } | ResponsesStreamingOutput::Unknown => {}
        }
    }
    Ok((texts.join("\n"), function_calls))
}

fn parse_input_text(command_str: &str, gpt_prefix_exists: bool) -> String {
    let text = if gpt_prefix_exists {
        let command_str = command_str.trim_start();
        let command_end = command_str
            .find(char::is_whitespace)
            .unwrap_or(command_str.len());
        &command_str[command_end..]
    } else {
        command_str
    };
    text.strip_prefix(' ').unwrap_or(text).to_string()
}

async fn build_responses_input<B: Bot>(
    bot: &B,
    msg: &crate::MessageEvent,
    input_text: &str,
    call_prefix: &str,
    messages: &[ThreadMessageType],
) -> anyhow::Result<Vec<ResponsesInput>> {
    let mut input = vec![];
    let mut current_files = msg.files.as_slice();

    for message in messages {
        let (is_bot, text, files, ts, blocks) = match message {
            ThreadMessageType::Unbroadcasted(message) => (
                message.bot_id.is_some(),
                &message.text,
                message.files.as_slice(),
                &message.ts,
                message.blocks.as_slice(),
            ),
            ThreadMessageType::Broadcasted(message) => (
                message.bot_id.is_some() || message.user.is_none(),
                &message.text,
                message.files.as_slice(),
                &message.ts,
                message.blocks.as_slice(),
            ),
            ThreadMessageType::None(_) => continue,
        };
        let (role, mut text, files) = if is_bot {
            let text = match blocks {
                [BlockElement::Section(label), BlockElement::Section(content), ..]
                    if label.text.text == "`ChatGPT`" =>
                {
                    content.text.text.clone()
                }
                [BlockElement::Section(label), BlockElement::RichText { .. }, ..]
                    if label.text.text == "`ChatGPT`" =>
                {
                    text.clone()
                }
                _ => continue,
            };
            ("assistant", text, &[][..])
        } else {
            ("user", text.clone(), files)
        };

        // The event is authoritative for the current request, including attachments
        // that may not have appeared in conversations.replies yet.
        if String::from(ts) == msg.ts {
            if current_files.is_empty() {
                current_files = files;
            }
            continue;
        }

        if role == "user" {
            if let Some(content) = text.strip_prefix(call_prefix.trim_end()) {
                if content.is_empty() || content.starts_with(char::is_whitespace) {
                    text = content.strip_prefix(' ').unwrap_or(content).to_string();
                }
            }
        }
        if text.is_empty() && !files.iter().any(SlackFile::is_image) {
            continue;
        }
        input.push(build_response_message(bot, role, text, files).await?);
    }

    input.push(build_response_message(bot, "user", input_text.to_string(), current_files).await?);
    Ok(input)
}

async fn build_response_message<B: Bot>(
    bot: &B,
    role: &str,
    text: String,
    files: &[SlackFile],
) -> anyhow::Result<ResponsesInput> {
    let content = if role == "user" && files.iter().any(SlackFile::is_image) {
        let mut parts = vec![];
        if !text.is_empty() {
            parts.push(ResponsesContentPart::InputText { text });
        }
        for file in files.iter().filter(|file| file.is_image()) {
            if !matches!(
                file.mimetype.as_str(),
                "image/png" | "image/jpeg" | "image/webp" | "image/gif"
            ) {
                bail!(
                    "Image {} has an unsupported format; use PNG, JPEG, WebP, or GIF",
                    file.id
                );
            }
            let bytes = bot
                .download_file(file)
                .await
                .with_context(|| format!("Failed to read image {}", file.id))?;
            let image_url = format!("data:{};base64,{}", file.mimetype, STANDARD.encode(bytes));
            parts.push(ResponsesContentPart::InputImage {
                image_url,
                detail: "auto",
            });
        }
        ResponsesMessageContent::Parts(parts)
    } else {
        ResponsesMessageContent::Text(text)
    };
    Ok(ResponsesInput::Message(OpenAIResponsesMessage {
        role: role.to_string(),
        content,
    }))
}

async fn get_function_call<B: Bot>(
    bot: &B,
    name: &str,
    call_id: &str,
    arguments: &str,
) -> anyhow::Result<ResponsesInput> {
    let arguments: HashMap<String, serde_json::Value> =
        serde_json::from_str(arguments).context("OpenAI returned invalid tool arguments")?;

    let tool_result = bot.call_mcp_tool(name, arguments).await?;

    let tool_output = ResponsesToolOutput::FunctionCallOutput {
        call_id: call_id.to_string(),
        output: tool_result,
    };

    Ok(ResponsesInput::FunctionCall(tool_output))
}

// TODO save bot as member?
struct GptMessageManager<'a> {
    channel: &'a String,
    ts: String,
    reply_event: Option<ReplyMessageEvent>,
    message: String,
}

impl<'a> GptMessageManager<'a> {
    pub fn new(channel: &'a String, reply_event: Option<ReplyMessageEvent>) -> Self {
        Self {
            channel,
            message: String::new(),
            ts: String::new(),
            reply_event,
        }
    }

    pub fn concat_message(&mut self, diff_message: &str) {
        self.message += diff_message;
    }

    pub async fn stream_message(
        &mut self,
        bot: &impl Bot,
        temp_message: Option<&str>,
    ) -> anyhow::Result<()> {
        if !self.ts.is_empty() {
            return self
                .edit_message(bot, &self.message, self.channel, &self.ts, temp_message)
                .await;
        } else {
            let sent = self
                .send_message(
                    bot,
                    &self.message,
                    self.channel,
                    &self.reply_event,
                    temp_message,
                )
                .await?;

            self.ts = String::from(&sent.ts.context("Slack returned no message timestamp")?);

            Ok(())
        }
    }

    pub async fn send_message_static(
        bot: &impl Bot,
        message: &str,
        channel: &str,
        reply_event: &Option<ReplyMessageEvent>,
    ) -> anyhow::Result<PostMessageResponse> {
        Self::send_message_with_status(bot, message, channel, reply_event, None).await
    }

    async fn send_message_with_status(
        bot: &impl Bot,
        message: &str,
        channel: &str,
        reply_event: &Option<ReplyMessageEvent>,
        status: Option<&str>,
    ) -> anyhow::Result<PostMessageResponse> {
        let blocks = Self::response_blocks(message, status);
        let text = Self::response_text(message, status);

        let sent = bot
            .send_message(
                channel,
                Message::BlocksWithText {
                    text: &text,
                    blocks: &blocks,
                },
                reply_event.clone(),
                None,
            )
            .await?;
        ensure!(
            sent.ok,
            "Slack rejected the ChatGPT message: {:?}",
            sent.error
        );
        Ok(sent)
    }

    async fn send_message(
        &self,
        bot: &impl Bot,
        message: &str,
        channel: &str,
        reply_event: &Option<ReplyMessageEvent>,
        status: Option<&str>,
    ) -> anyhow::Result<PostMessageResponse> {
        Self::send_message_with_status(bot, message, channel, reply_event, status).await
    }

    async fn edit_message(
        &self,
        bot: &impl Bot,
        message: &str,
        channel: &str,
        ts: &str,
        status: Option<&str>,
    ) -> anyhow::Result<()> {
        let blocks = Self::response_blocks(message, status);
        let text = Self::response_text(message, status);

        let sent = bot
            .edit_message(
                channel,
                Message::BlocksWithText {
                    text: &text,
                    blocks: &blocks,
                },
                ts,
            )
            .await?;
        ensure!(
            sent.ok,
            "Slack rejected the ChatGPT update: {:?}",
            sent.error
        );
        Ok(())
    }

    fn response_text<'b>(message: &'b str, status: Option<&str>) -> Cow<'b, str> {
        match status {
            Some(status) if !message.is_empty() => {
                Cow::Owned(format!("{}\n\n{}", message, status.trim_start()))
            }
            Some(status) => Cow::Owned(status.trim_start().to_string()),
            None => Cow::Borrowed(message),
        }
    }

    fn response_blocks(message: &str, status: Option<&str>) -> [BlockElement; 2] {
        let mut answer = crate::slack::markdown_to_rich_text(message);
        if let Some(status) = status {
            let footer = crate::slack::markdown_to_rich_text(status.trim_start());
            if message.is_empty() {
                answer = footer;
            } else if let (
                BlockElement::RichText { elements, .. },
                BlockElement::RichText {
                    elements: footer, ..
                },
            ) = (&mut answer, footer)
            {
                elements.extend(footer);
            }
        }
        [
            BlockElement::Section(SectionBlock::new_markdown("`ChatGPT`")),
            answer,
        ]
    }
}
