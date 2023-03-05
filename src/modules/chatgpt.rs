use log::debug;
use serde::{Deserialize, Serialize};

use crate::{slack, Message};

#[derive(Serialize)]
struct OpenAIChatCompletionMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAIChatCompletionBody {
    model: String,
    messages: Vec<OpenAIChatCompletionMessage>,
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
    finish_reason: String,
    index: i32,
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

    debug!("GPT: bot command full text = {:?}", &msg.text);

    let command_str = msg.text.replace(&slack_bot_format, "");

    let slices = command_str.split_whitespace().collect::<Vec<&str>>();

    if slices.is_empty() {
        return Ok(());
    }

    let call_type = slices[0];
    debug!("call_type: {:?}", call_type);

    if call_type != "gpt" && call_type != "chatgpt" {
        return Ok(());
    }

    let input_text = slices.iter().cloned().skip(1).collect::<Vec<_>>().join(" ");

    let req_body = OpenAIChatCompletionBody {
        model: "gpt-3.5-turbo".to_string(),
        messages: vec![OpenAIChatCompletionMessage {
            role: "user".to_string(),
            content: input_text,
        }],
    };

    let req_client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
        .build()?;

    let res_result = req_client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(bot.openai_key())
        .json(&req_body)
        .send()
        .await;

    if res_result.is_err() {
        let debug_str = "OpenAI API call failed";
        debug!("{}", debug_str);

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: debug_str.to_string(),
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                })]),
            )
            .await
            .and(Ok(()));
    }

    let res = res_result.unwrap();
    let res_len = res.content_length().unwrap_or(0);

    let res_bytes = res.bytes().await;

    if res_bytes.is_err() {
        let debug_str = format!("OpenAI result bytes error: {}", res_len);
        debug!("{}", debug_str);

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: debug_str,
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                })]),
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
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: debug_str,
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                })]),
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

    let output_text = format!("> {}", res_text);

    let gpt_name_block = slack::BlockElement::Section(slack::SectionBlock {
        text: slack::TextObject {
            ty: slack::TextObjectType::Markdown,
            text: "`ChatGPT`".to_string(),
            emoji: None,
            verbatim: None,
        },
        block_id: None,
        fields: None,
    });

    let gpt_answer_block = slack::BlockElement::Section(slack::SectionBlock {
        text: slack::TextObject {
            ty: slack::TextObjectType::Markdown,
            text: output_text,
            emoji: None,
            verbatim: None,
        },
        block_id: None,
        fields: None,
    });

    return bot
        .send_message(
            &msg.channel,
            Message::Blocks(&[gpt_name_block, gpt_answer_block]),
        )
        .await
        .and(Ok(()));
}
