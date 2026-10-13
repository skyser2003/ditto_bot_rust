use std::{borrow::Cow, collections::HashMap, env};

use anyhow::{bail, Context};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures::StreamExt;
use log::{debug, error};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, PostMessageResponse, SectionBlock, SlackFile, ThreadMessageType},
    Bot, Message, ReplyMessageEvent,
};

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

    let command_str = msg.text.replace(&slack_bot_format, "");

    let slices = command_str.split_whitespace().collect::<Vec<&str>>();

    if slices.is_empty() && !msg.files.iter().any(SlackFile::is_image) {
        return Ok(());
    }

    let call_type = slices.first().copied().unwrap_or("");

    let gpt_split = call_type.split("gpt").collect::<Vec<_>>();

    let gpt_prefix_exists = call_type.starts_with("gpt");

    let call_prefix = if !gpt_prefix_exists {
        format!("{} ", slack_bot_format)
    } else {
        format!("{} {} ", slack_bot_format, call_type)
    };

    debug!("GPT: bot command full text = {:?}", msg.text);

    let input_text = parse_input_text(&command_str, gpt_prefix_exists);

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
        match build_responses_input(bot, msg, &input_text, &call_prefix, messages).await {
            Ok(input) => input,
            Err(error) => {
                let text = format!("Unable to read an attached image: {:#}", error);
                bot.send_message(&msg.channel, Message::Text(&text), reply_event, None)
                    .await?;
                return Ok(());
            }
        };

    let chat_url = "https://api.openai.com/v1/responses";

    let openai_builder = openai_req
        .post(chat_url)
        .bearer_auth(bot.openai_key())
        .json(&openai_body);

    if stream_mode {
        let mut gpt_message = GptMessageManager::new(&msg.channel, reply_event.clone());
        let mut initial_received = false;

        let mut openai_sse = EventSource::new(openai_builder)?;

        while let Some(event) = openai_sse.next().await {
            match event {
                Ok(Event::Open) => {
                    debug!("OpenAI SSE opened");
                }
                Ok(Event::Message(event)) => {
                    let data = event.data.clone();

                    let sse_res = serde_json::from_str::<ResponsesStreamingResponse>(&data);

                    if sse_res.is_err() {
                        error!("OpenAI SSE json parsing failed: {:?}", data);
                        continue;
                    }

                    let sse_res = sse_res.unwrap();

                    match sse_res {
                        ResponsesStreamingResponse::Delta { item_id: _, delta } => {
                            debug!("OpenAI SSE delta: {:?}", delta);

                            if !initial_received {
                                initial_received = true;

                                match gpt_message
                                    .stream_message(bot, Some("`Receiving...`"))
                                    .await
                                {
                                    Ok(_) => {}
                                    Err(e) => {
                                        error!("OpenAI SSE stream message sending failed: {:?}", e);
                                    }
                                }
                            }

                            gpt_message.concat_message(&delta);

                            if ![",", ".", "?", "!", "\n"].contains(&delta.as_str()) {
                                continue;
                            }

                            let sent = gpt_message.stream_message(bot, Some(" `[continue]`")).await;

                            if sent.is_err() {
                                error!("OpenAI SSE stream message sending failed: {:?}", sent);

                                return Ok(());
                            }
                        }
                        ResponsesStreamingResponse::Completed { response } => {
                            debug!("OpenAI SSE received {}", data);

                            let mut function_calls = vec![];

                            for item in response.output {
                                match item {
                                    ResponsesStreamingOutput::Message {
                                        id: _,
                                        status: _,
                                        role: _,
                                        content: _,
                                    } => {}
                                    ResponsesStreamingOutput::Reasoning { id: _ } => {}
                                    ResponsesStreamingOutput::FunctionCall {
                                        id: _,
                                        status: _,
                                        arguments,
                                        call_id,
                                        name,
                                    } => {
                                        let function_call =
                                            get_function_call(bot, &name, &call_id, &arguments)
                                                .await?;

                                        function_calls.push(function_call);
                                    }
                                    ResponsesStreamingOutput::Unknown => {
                                        error!("OpenAI SSE unknown output: {:?}", data);
                                    }
                                }
                            }

                            if function_calls.is_empty() {
                                gpt_message.concat_message(&format!(" `{}`", "[DONE]"));

                                let sent = gpt_message.stream_message(bot, None).await;

                                if sent.is_err() {
                                    error!("OpenAI SSE stream {} sending failed: {:?}", data, sent);
                                }

                                break;
                            } else {
                                openai_body.previous_response_id = Some(response.id);
                                openai_body.input.extend(function_calls);

                                let builder = openai_req
                                    .post(chat_url)
                                    .bearer_auth(bot.openai_key())
                                    .json(&openai_body);

                                openai_sse = EventSource::new(builder)?;
                            }
                        }
                        ResponsesStreamingResponse::Created
                        | ResponsesStreamingResponse::InProgress
                        | ResponsesStreamingResponse::OutputItemAdded
                        | ResponsesStreamingResponse::OutputItemDone
                        | ResponsesStreamingResponse::OutputTextDone
                        | ResponsesStreamingResponse::ContentPartAdded
                        | ResponsesStreamingResponse::ContentPartDone
                        | ResponsesStreamingResponse::FunctionCallArgumentsDelta
                        | ResponsesStreamingResponse::FunctionCallArgumentsDone => {
                            // Ignore
                        }
                        ResponsesStreamingResponse::Unknown => {
                            error!("OpenAI SSE unknown response: {:?}", data);
                        }
                    }
                }
                Err(e) => {
                    match e {
                        reqwest_eventsource::Error::StreamEnded => {
                            debug!("OpenAI SSE stream ended");
                        }
                        reqwest_eventsource::Error::InvalidStatusCode(code, err_res) => {
                            error!("OpenAI SSE error code: {}", code);

                            match err_res.text().await {
                                Ok(body) => error!("OpenAI SSE error res body: {}", body),
                                Err(e) => error!("Failed to read error res body: {:?}", e),
                            }
                        }
                        _ => {
                            error!("OpenAI SSE error: {:?}", e);
                        }
                    }

                    break;
                }
            }
        }

        Ok(())
    } else {
        let mut openai_res = openai_builder.send().await;

        loop {
            if openai_res.is_err() {
                let debug_str = "OpenAI API call failed";
                debug!("{}", debug_str);

                return bot
                    .send_message(
                        &msg.channel,
                        Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(
                            debug_str,
                        ))]),
                        reply_event,
                        None,
                    )
                    .await
                    .and(Ok(()));
            }

            let res = openai_res.unwrap();
            let res_len = res.content_length().unwrap_or(0);

            let res_bytes = res.bytes().await;

            if res_bytes.is_err() {
                let debug_str = format!("OpenAI result bytes error: {}", res_len);
                debug!("{}", debug_str);

                return bot
                    .send_message(
                        &msg.channel,
                        Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(
                            &debug_str,
                        ))]),
                        reply_event,
                        None,
                    )
                    .await
                    .and(Ok(()));
            }

            let res_bytes = res_bytes.unwrap();

            let res_body_result = serde_json::from_slice::<ResponsesCompletedResponse>(&res_bytes);

            if res_body_result.is_err() {
                let debug_str = format!(
                    "OpenAI result json parsing failed: {:?}",
                    String::from_utf8(res_bytes.to_vec()).unwrap()
                );

                error!("{}", debug_str);

                return bot
                    .send_message(
                        &msg.channel,
                        Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(
                            &debug_str,
                        ))]),
                        reply_event,
                        None,
                    )
                    .await
                    .and(Ok(()));
            }

            let res_body = res_body_result.unwrap();

            let mut res_texts = vec![];
            let mut function_calls = vec![];

            for output in &res_body.output {
                match output {
                    ResponsesStreamingOutput::Message {
                        id: _,
                        status: _,
                        role: _,
                        content,
                    } => res_texts.push(content[0].text.clone()),
                    ResponsesStreamingOutput::Reasoning { id: _ } => {}
                    ResponsesStreamingOutput::FunctionCall {
                        id: _,
                        status: _,
                        arguments,
                        call_id,
                        name,
                    } => {
                        let function_call =
                            get_function_call(bot, name, call_id, arguments).await?;

                        function_calls.push(function_call);
                    }
                    ResponsesStreamingOutput::Unknown => {
                        error!("OpenAI SSE unknown output: {:?}", res_body);
                    }
                }
            }

            if !function_calls.is_empty() {
                openai_body.previous_response_id = Some(res_body.id);
                openai_body.input.extend(function_calls);

                let openai_builder = openai_req
                    .post(chat_url)
                    .bearer_auth(bot.openai_key())
                    .json(&openai_body);

                openai_res = openai_builder.send().await;

                continue;
            } else {
                let res_text = res_texts.join("\n");

                return GptMessageManager::send_message_static(
                    bot,
                    &res_text,
                    &msg.channel,
                    &reply_event,
                )
                .await
                .and(Ok(()));
            }
        }
    }
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
        serde_json::from_str(arguments).unwrap_or_else(|_| HashMap::new());

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
        let mut message = Cow::from(&self.message);

        if let Some(temp_message) = temp_message {
            message += temp_message;
        }

        if !self.ts.is_empty() {
            return self
                .edit_message(bot, &message, self.channel, &self.ts)
                .await;
        } else {
            let sent = self
                .send_message(bot, &message, self.channel, &self.reply_event)
                .await;

            if sent.is_err() {
                return Err(sent.err().unwrap());
            }

            self.ts = String::from(&sent.unwrap().ts.unwrap());

            Ok(())
        }
    }

    pub async fn send_message_static(
        bot: &impl Bot,
        message: &str,
        channel: &str,
        reply_event: &Option<ReplyMessageEvent>,
    ) -> anyhow::Result<PostMessageResponse> {
        let gpt_name_block = BlockElement::Section(SectionBlock::new_markdown("`ChatGPT`"));
        let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(message));

        let blocks = [gpt_name_block, gpt_answer_block];

        let sent = bot
            .send_message(channel, Message::Blocks(&blocks), reply_event.clone(), None)
            .await;

        sent
    }

    async fn send_message(
        &self,
        bot: &impl Bot,
        message: &str,
        channel: &str,
        reply_event: &Option<ReplyMessageEvent>,
    ) -> anyhow::Result<PostMessageResponse> {
        Self::send_message_static(bot, message, channel, reply_event).await
    }

    async fn edit_message(
        &self,
        bot: &impl Bot,
        message: &str,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<()> {
        let gpt_name_block = BlockElement::Section(SectionBlock::new_markdown("`ChatGPT`"));
        let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(message));

        let blocks = [gpt_name_block, gpt_answer_block];

        let sent = bot
            .edit_message(channel, Message::Blocks(&blocks), ts)
            .await;

        if sent.is_err() {
            error!("Edit message failed: {:?}", sent.err());
        }

        Ok(())
    }
}
