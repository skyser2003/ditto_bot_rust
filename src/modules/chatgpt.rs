use std::env;

use futures::StreamExt;
use log::debug;
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, SectionBlock, ThreadMessageType},
    Message, ReplyMessageEvent,
};

#[derive(Debug, Serialize)]
struct OpenAIChatCompletionMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAIChatCompletionBody {
    model: String,
    messages: Vec<OpenAIChatCompletionMessage>,
    stream: bool,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ResChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    usage: ResUsage,
    choices: Vec<ResChoice>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SseChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<SseChoice>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ResUsage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ResChoice {
    message: ResMessage,
    finish_reason: Option<String>,
    index: i32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SseChoice {
    delta: SseChoiceDelta,
    finish_reason: Option<String>,
    index: i32,
}

#[derive(Debug, Deserialize, Clone)]
struct SseChoiceDelta {
    content: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ResMessage {
    role: String,
    content: String,
}

pub async fn handle<'a, B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
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

    if call_type != "gpt" {
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

    let conv_fut = bot.get_conversation_relies(&msg.channel, thread_ts.as_str());
    let conv_result = conv_fut.await;

    let stream_mode_str = env::var("USE_GPT_STREAM").unwrap_or("true".to_string());
    let stream_mode_str = stream_mode_str.to_lowercase();

    let stream_mode = if stream_mode_str == "true" || stream_mode_str == "1" {
        true
    } else {
        false
    };

    let mut openai_body = OpenAIChatCompletionBody {
        model: "gpt-3.5-turbo".to_string(),
        messages: vec![],
        stream: stream_mode,
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
                    .messages
                    .push(OpenAIChatCompletionMessage { role, content });
            });
        }
    };

    if openai_body.messages.len() == 0 {
        debug!("Error! no thread found");

        openai_body.messages = vec![OpenAIChatCompletionMessage {
            role: "user".to_string(),
            content: input_text,
        }];
    }

    let reply_event = Some(ReplyMessageEvent {
        msg: thread_ts,
        broadcast: true,
    });

    let chat_url = "https://api.openai.com/v1/chat/completions";

    let openai_builder = openai_req
        .post(chat_url)
        .bearer_auth(bot.openai_key())
        .json(&openai_body);

    if stream_mode {
        let mut openai_sse = EventSource::new(openai_builder.try_clone().unwrap())?;

        let mut message_ts = None;

        let mut full_message = String::new();

        while let Some(event) = openai_sse.next().await {
            match event {
                Ok(Event::Open) => {
                    debug!("OpenAI SSE opened");
                }
                Ok(Event::Message(event)) => {
                    let data = event.data;

                    if data == "[DONE]" {
                        debug!("OpenAI SSE received [DONE]");

                        full_message += " [DONE]";

                        send_gpt_edit_message(
                            bot,
                            &full_message,
                            &msg.channel,
                            &String::from(&message_ts.clone().unwrap()),
                        )
                        .await;

                        break;
                    }

                    let sse_res = serde_json::from_str::<SseChatCompletion>(&data);

                    if sse_res.is_err() {
                        debug!("OpenAI SSE json parsing failed: {:?}", data);
                        continue;
                    }

                    let sse_res_json = sse_res.unwrap();
                    let message_opt = sse_res_json.choices[0].delta.clone().content;

                    if message_opt.is_none() {
                        continue;
                    }

                    if message_ts == None {
                        full_message = message_opt.unwrap();

                        let sent = bot
                            .send_message(
                                &msg.channel,
                                Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(
                                    &full_message,
                                ))]),
                                reply_event.clone(),
                            )
                            .await;

                        if sent.is_err() {
                            debug!("OpenAI SSE stream message sending failed: {:?}", sent);

                            return Ok(());
                        }

                        message_ts = sent.unwrap().ts;
                    } else {
                        if message_opt.is_none() {
                            continue;
                        }

                        let diff_message = message_opt.unwrap();
                        full_message += &diff_message;

                        send_gpt_edit_message(
                            bot,
                            &full_message,
                            &msg.channel,
                            &String::from(&message_ts.clone().unwrap()),
                        )
                        .await;
                    }
                }
                Err(e) => {
                    debug!("OpenAI SSE error: {:?}", e);
                    break;
                }
            }
        }

        Ok(())
    } else {
        let openai_res = openai_builder.send().await;

        if openai_res.is_err() {
            let debug_str = "OpenAI API call failed";
            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(debug_str))]),
                    reply_event,
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
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(&debug_str))]),
                    reply_event,
                )
                .await
                .and(Ok(()));
        }

        let res_bytes = res_bytes.unwrap();

        let res_body_result = serde_json::from_slice::<ResChatCompletion>(&res_bytes);

        if res_body_result.is_err() {
            let debug_str = format!(
                "OpenAI result json parsing failed: {:?}",
                String::from_utf8(res_bytes.to_vec()).unwrap()
            );

            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(&debug_str))]),
                    reply_event,
                )
                .await
                .and(Ok(()));
        }

        let res_body = res_body_result.unwrap();

        let res_text = if res_body.choices.len() == 0 {
            "ditto_bot Error: "
        } else {
            &res_body.choices[0].message.content
        };

        let res_text = res_text.trim_start();

        let gpt_name_block = BlockElement::Section(SectionBlock::new_markdown("`ChatGPT`"));
        let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(res_text));

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[gpt_name_block, gpt_answer_block]),
                reply_event,
            )
            .await
            .and(Ok(()));
    }
}

async fn send_gpt_edit_message<B: crate::Bot>(bot: &B, message: &str, channel: &str, ts: &str) {
    let gpt_name_block = BlockElement::Section(SectionBlock::new_markdown("`ChatGPT`"));
    let gpt_answer_block = BlockElement::Section(SectionBlock::new_markdown(&message));

    let blocks = [gpt_name_block, gpt_answer_block];

    let sent = bot
        .edit_message(channel, Message::Blocks(&blocks), ts)
        .await;

    if sent.is_err() {
        debug!("Edit message failed: {:?}", sent.err());
    }
}
