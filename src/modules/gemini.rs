use std::{borrow::Cow, env};

use futures::StreamExt;
use log::{debug, error, info};
use reqwest_eventsource::{Event, EventSource};
use serde::{Deserialize, Serialize};

use crate::{
    slack::{BlockElement, PostMessageResponse, SectionBlock, ThreadMessageType},
    Bot, Message, ReplyMessageEvent,
};

#[derive(Debug, Serialize, Deserialize)]
struct GeminiChatStreamText {
    text: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiChatStreamMessage {
    role: String,
    parts: Vec<GeminiChatStreamText>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiChatStreamBody {
    contents: Vec<GeminiChatStreamMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiChatGenerationConfig>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiChatGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_k: Option<i32>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResChatCompletion {
    candidates: Vec<ResChatCandidate>,
    prompt_feedback: Option<ResPromptFeedback>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResChatCandidate {
    content: GeminiChatStreamMessage,
    finish_reason: String,
    index: i32,
    safety_ratings: Vec<ResChatSafetyRating>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ResChatSafetyRating {
    category: String,
    probability: String,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]

struct ResPromptFeedback {
    safety_ratings: Vec<ResChatSafetyRating>,
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

    let gemini_split = call_type.split("gemini").collect::<Vec<_>>();

    if gemini_split[0] != "" {
        return Ok(());
    }

    let temperature = gemini_split[1].parse::<f32>().unwrap_or(0.0);

    let call_prefix = format!("{} {} ", slack_bot_format, call_type);

    debug!("Gemini: bot command full text = {:?}", &msg.text);

    let input_text = slices.iter().cloned().skip(1).collect::<Vec<_>>().join(" ");

    let gemini_req = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
        .build()?;

    let thread_ts = if let Some(thread_ts) = msg.thread_ts.clone() {
        thread_ts
    } else {
        msg.ts.clone()
    };

    let conv_fut = bot.get_conversation_relies(&msg.channel, thread_ts.as_str());
    let conv_result = conv_fut.await;

    let stream_mode_str = env::var("USE_GEMINI_STREAM").unwrap_or("true".to_string());
    let stream_mode_str = stream_mode_str.to_lowercase();

    let stream_mode = if stream_mode_str == "true" || stream_mode_str == "1" {
        true
    } else {
        false
    };

    let gemini_model = env::var("GEMINI_MODEL").unwrap_or("gemini-pro".to_string());

    let mut gemini_body = GeminiChatStreamBody {
        contents: vec![],
        generation_config: Some(GeminiChatGenerationConfig {
            stop_sequences: None,
            temperature: Some(temperature),
            max_output_tokens: None,
            top_p: None,
            top_k: None,
        }),
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
                            let speaker = "model";

                            if val.blocks.len() < 2 {
                                return;
                            }

                            let mut text = val.text.clone();

                            let mut is_valid_response = false;

                            if let BlockElement::Section(section) = &val.blocks[0] {
                                if section.text.text == "`Gemini`" {
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

                gemini_body.contents.push(GeminiChatStreamMessage {
                    role,
                    parts: vec![GeminiChatStreamText { text: content }],
                });
            });
        }
    };

    if gemini_body.contents.len() == 0 {
        error!("Error! no thread found");

        gemini_body.contents = vec![GeminiChatStreamMessage {
            role: "user".to_string(),
            parts: vec![GeminiChatStreamText { text: input_text }],
        }];
    }

    let reply_event = Some(ReplyMessageEvent {
        msg: thread_ts,
        broadcast: true,
    });

    let chat_url = if stream_mode {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent",
            gemini_model
        )
    } else {
        format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            gemini_model
        )
    };

    let gemini_builder = gemini_req
        .post(chat_url)
        .header("x-goog-api-key", bot.gemini_key())
        .json(&gemini_body);

    if stream_mode {
        let mut gemini_message = GeminiMessageManager::new(&msg.channel, reply_event.clone());
        let mut initial_received = false;

        let gemini_res = gemini_builder.send().await;

        if gemini_res.is_err() {
            let debug_str = "Gemini API call failed";
            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(debug_str))]),
                    reply_event,
                    None,
                )
                .await
                .and(Ok(()));
        }

        let mut bytes_stream = gemini_res.unwrap().bytes_stream();

        while let Some(Ok(next_bytes)) = bytes_stream.next().await {
            let mut data = String::from_utf8(next_bytes.to_vec()).unwrap();

            if data.starts_with("[") || data.starts_with(",") {
                data = data[1..].to_string();
            } else if data.ends_with("]") && data.len() > 2 {
                data = data[..data.len() - 2].to_string();
            }

            let sse_res = serde_json::from_str::<ResChatCompletion>(&data);

            if sse_res.is_err() {
                error!("Gemini SSE json parsing failed: {:?}", data);
                continue;
            }

            let stream_res_json = sse_res.unwrap();
            let diff_message = stream_res_json.candidates[0].content.parts[0].text.clone();

            if !initial_received {
                initial_received = true;

                match gemini_message
                    .stream_message(bot, Some("`Receiving...`"))
                    .await
                {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Gemini SSE stream message sending failed: {:?}", e);
                    }
                }
            }

            gemini_message.concat_message(&diff_message);

            let sent = gemini_message
                .stream_message(bot, Some(" `[continue]`"))
                .await;

            if sent.is_err() {
                error!("Gemini SSE stream message sending failed: {:?}", sent);

                return Ok(());
            }
        }

        let done_message = "[DONE]";

        gemini_message.concat_message(&format!(" `{}`", done_message));

        let sent = gemini_message.stream_message(bot, None).await;

        if sent.is_err() {
            error!(
                "Gemini SSE stream {} sending failed: {:?}",
                done_message, sent
            );
        }

        Ok(())
    } else {
        let gemini_res = gemini_builder.send().await;

        if gemini_res.is_err() {
            let debug_str = "Gemini API call failed";
            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(debug_str))]),
                    reply_event,
                    None,
                )
                .await
                .and(Ok(()));
        }

        let res = gemini_res.unwrap();
        let res_len = res.content_length().unwrap_or(0);

        let res_bytes = res.bytes().await;

        if res_bytes.is_err() {
            let debug_str = format!("Gemini result bytes error: {}", res_len);
            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(&debug_str))]),
                    reply_event,
                    None,
                )
                .await
                .and(Ok(()));
        }

        let res_bytes = res_bytes.unwrap();
        info!("{:?}", res_bytes);

        let res_body_result = serde_json::from_slice::<ResChatCompletion>(&res_bytes);

        if res_body_result.is_err() {
            let debug_str = format!(
                "Gemini result json parsing failed: {:?}",
                String::from_utf8(res_bytes.to_vec()).unwrap()
            );

            debug!("{}", debug_str);

            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[BlockElement::Section(SectionBlock::new_text(&debug_str))]),
                    reply_event,
                    None,
                )
                .await
                .and(Ok(()));
        }

        let res_body = res_body_result.unwrap();

        let res_text = if res_body.candidates.len() == 0 {
            "ditto_bot Error: "
        } else {
            &res_body.candidates[0].content.parts[0].text
        };

        let res_text = res_text.trim_start();

        GeminiMessageManager::send_message_static(bot, &res_text, &msg.channel, &reply_event)
            .await
            .and(Ok(()))
    }
}

// TODO save bot as member?
struct GeminiMessageManager<'a> {
    channel: &'a String,
    ts: String,
    reply_event: Option<ReplyMessageEvent>,
    message: String,
}

impl<'a> GeminiMessageManager<'a> {
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
        let gemini_name_block = BlockElement::Section(SectionBlock::new_markdown("`Gemini`"));
        let gemini_answer_block = BlockElement::Section(SectionBlock::new_markdown(&message));

        let blocks = [gemini_name_block, gemini_answer_block];

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
        let gemini_name_block = BlockElement::Section(SectionBlock::new_markdown("`Gemini`"));
        let gemini_answer_block = BlockElement::Section(SectionBlock::new_markdown(&message));

        let blocks = [gemini_name_block, gemini_answer_block];

        let sent = bot
            .edit_message(channel, Message::Blocks(&blocks), ts)
            .await;

        if sent.is_err() {
            error!("Edit message failed: {:?}", sent.err());
        }

        Ok(())
    }
}
