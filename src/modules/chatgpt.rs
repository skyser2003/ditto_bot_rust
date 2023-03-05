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

#[derive(Deserialize)]
struct ResChatCompletion {
    _id: String,
    _object: String,
    _created: i64,
    _model: String,
    _usage: ResUsage,
    choices: Vec<ResChoice>,
}

#[derive(Deserialize)]
struct ResUsage {
    _prompt_tokens: i32,
    _completion_tokens: i32,
    _total_tokens: i32,
}

#[derive(Deserialize)]
struct ResChoice {
    message: ResMessage,
    _finish_reason: String,
    _index: i32,
}

#[derive(Deserialize)]
struct ResMessage {
    _role: String,
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
        debug!("OpenAI API call failed");

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: "OpenAI API call failed".to_string(),
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
    let res_body_result = res.json::<ResChatCompletion>().await;

    if res_body_result.is_err() {
        debug!("OpenAI result json parsing failed");

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: "OpenAI result json parsing failed".to_string(),
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
