use std::{borrow::Cow, collections::HashMap, env};

use futures::StreamExt;
use log::{debug, error};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, PostMessageResponse, SectionBlock, ThreadMessageType},
    Bot, Message, ReplyMessageEvent,
};
#[derive(Debug, Serialize)]
#[serde(untagged)]
#[serde(rename_all = "snake_case")]
enum ResponsesInput {
    Text(OpenAIChatCompletionMessage),
    FunctionCall(ResponsesToolOutput),
}

#[derive(Debug, Serialize)]
struct OpenAIChatCompletionMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAIResponsesBody {
    model: String,
    input: Vec<ResponsesInput>,
    temperature: f32,
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
    OutputItemDone { item: ResponsesStreamingOutput },
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
        id: String,
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
    #[serde(rename = "type")]
    type_field: String,
    text: String,
}

#[derive(Deserialize)]
pub struct ResponsesStreamingOutputContent {
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

pub async fn handle<'a, B: Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    let slack_bot_format = format!("<@{}>", bot.bot_id());
    let is_bot_command = msg.text.contains(&slack_bot_format);

    if !is_bot_command {
        return Ok(());
    }

    let command_str = msg.text.replace(&slack_bot_format, "");

    let slices = command_str.split_whitespace().collect::<Vec<&str>>();

    if slices.is_empty() {
        return Ok(());
    }

    let call_type = slices[0];

    let gpt_split = call_type.split("gpt").collect::<Vec<_>>();

    if gpt_split[0] != "" {
        return Ok(());
    }

    let call_prefix = format!("{} {} ", slack_bot_format, call_type);

    debug!("GPT: bot command full text = {:?}", &msg.text);

    let input_text = slices.iter().cloned().skip(1).collect::<Vec<_>>().join(" ");

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
    let temperature = if openai_model.starts_with("o") {
        1.0
    } else {
        gpt_split[1].parse::<f32>().unwrap_or(0.0)
    };

    let mut tools = vec![];

    if !openai_model.starts_with("o") {
        tools.push(OpenAIResponsesTool::WebSearch);
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

    if let Ok(conv_res) = conv_result {
        if let Some(messages) = conv_res.messages {
            messages.iter().for_each(|msg| {
                let (role, mut content) = match msg {
                    ThreadMessageType::Unbroadcasted(val) => ("user", val.text.clone()),
                    ThreadMessageType::Broadcasted(val) => {
                        if val.user.is_some() {
                            ("user", val.text.clone())
                        } else {
                            let speaker = "assistant";

                            if val.blocks.len() < 2 {
                                return;
                            }

                            let mut text = val.text.clone();

                            let mut is_valid_response = false;

                            if let BlockElement::Section(section) = &val.blocks[0] {
                                if section.text.text == "`ChatGPT`" {
                                    if let BlockElement::Section(real_text_section) = &val.blocks[1]
                                    {
                                        text = real_text_section.text.text.clone();
                                        is_valid_response = true;
                                    }
                                }
                            }

                            if !is_valid_response {
                                return;
                            }

                            (speaker, text)
                        }
                    }
                    ThreadMessageType::None(_) => return,
                };

                if role == "user" {
                    let call_split = content.split(&call_prefix).collect::<Vec<_>>();

                    if call_split.len() == 2 {
                        content = call_split[1].to_string();
                    }
                }

                let role = role.to_string();

                openai_body
                    .input
                    .push(ResponsesInput::Text(OpenAIChatCompletionMessage {
                        role,
                        content,
                    }));
            });
        }
    };

    if openai_body.input.len() == 0 {
        error!("Error! no thread found");

        openai_body.input = vec![ResponsesInput::Text(OpenAIChatCompletionMessage {
            role: "user".to_string(),
            content: input_text,
        })];
    }

    let reply_event = Some(ReplyMessageEvent {
        msg: thread_ts,
        broadcast: true,
    });

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
            match &event {
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
                        ResponsesStreamingResponse::OutputItemDone { item: _ } => {}
                        ResponsesStreamingResponse::Created
                        | ResponsesStreamingResponse::InProgress
                        | ResponsesStreamingResponse::OutputItemAdded
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
                        _ => {
                            error!("OpenAI SSE body: {:?}", serde_json::to_string(&openai_body));
                            error!("OpenAI SSE event: {:?}", event);
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

async fn get_function_call<B: Bot>(
    bot: &B,
    name: &String,
    call_id: &String,
    arguments: &String,
) -> anyhow::Result<ResponsesInput> {
    let arguments: HashMap<String, serde_json::Value> =
        serde_json::from_str(&arguments).unwrap_or_else(|_| HashMap::new());

    let tool_result = bot.call_mcp_tool(name, arguments).await?;

    let tool_output = ResponsesToolOutput::FunctionCallOutput {
        call_id: call_id.clone(),
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

    pub fn concat_message(&mut self, diff_message: &String) {
        self.message += diff_message;
    }

    pub async fn stream_message(
        &mut self,
        bot: &impl Bot,
        temp_message: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut message = Cow::from(&self.message);

        match temp_message {
            Some(temp_message) => message += temp_message,
            None => {}
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
        let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(&message));

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
        let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(&message));

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
