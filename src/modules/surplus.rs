use crate::slack;
use log::debug;
use redis::{Commands, Connection};
use serde_json::Value;
use std::borrow::Cow;
use std::cmp::{max, min};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn handle<'a>(
    text: &String,
    user: &String,
    blocks: &mut Vec<slack::BlockElement<'a>>,
    conn: &mut Connection,
    bot_id: &String,
    bot_token: &String,
) -> redis::RedisResult<()> {
    let slices = text.split_whitespace().collect::<Vec<&str>>();
    let slack_bot_format = format!("<@{}>", bot_id);

    log::debug!("full_text: {:?}", text);

    if 2 <= slices.len() && slices[0] == slack_bot_format {
        let call_type = slices[1];

        log::debug!("call_type: {:?}", call_type);

        if call_type == "잉여" {
            let mut table = std::collections::HashMap::<String, i32>::new();

            let records: Vec<String> = conn.zrangebyscore("ditto-archive", "-inf", "+inf").unwrap();

            if records.len() == 0 {
                blocks.push(slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::PlainText,
                        text: Cow::from("[There is no chat record.]"),
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                }));

                return Ok(());
            }

            for record in records {
                let user_id = record.split(":").nth(1).unwrap().to_string();

                let prev_count = table.get(&user_id);
                let next_count = match prev_count {
                    Some(val) => val + 1,
                    None => 1,
                };

                table.insert(user_id, next_count);
            }

            let mut vec_table = Vec::<(String, i32)>::new();

            for pair in table.iter_mut() {
                vec_table.push((pair.0.to_string(), *pair.1));
            }

            vec_table.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            if slices.len() <= 2 || slices[2] != "all" {
                // Top 5 user's chat count only
                vec_table.truncate(5);
            }

            let users_list = get_users_list(bot_token);

            let mut vec_bar = Vec::<String>::new();

            for pair in vec_table {
                let user_bar = format!("*{:}:\n\t*{:}", pair.0, generate_bar(pair.1, 2));

                vec_bar.push(user_bar);
            }

            let graph_text = vec_bar.join("\n");

            blocks.push(slack::BlockElement::Section(slack::SectionBlock {
                text: slack::TextObject {
                    ty: slack::TextObjectType::Markdown,
                    text: Cow::from(graph_text),
                    emoji: None,
                    verbatim: None,
                },
                block_id: None,
                fields: None,
            }));

            debug!("Blocks: {:?}", blocks);
        }
    } else {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let score = now.as_millis();
        let member = format!("{}:{}", score, user);

        conn.zadd("ditto-archive", member, score as i64)?;
    }

    Ok(())
}

fn generate_bar(chat_count: i32, level: usize) -> String {
    let characters = ["", "▌", "█"];
    let steps = max(min(level, characters.len() - 1), 1);

    let n = ((chat_count / 1000) as f32).round() as i32;
    let graph_char = characters[steps];

    let length = n / steps as i32;

    let mut graph_str = (0..length).map(|_| graph_char).collect::<String>();

    graph_str.push_str(characters[(n % steps as i32) as usize]);

    graph_str
}

async fn get_users_list(bot_token: &String) -> anyhow::Result<String> {
    let link = "https://slack.com/api/users.list";

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:94.0) Gecko/20100101 Firefox/94.0")
        .build()?;

    let res = client
        .get(link)
        .header("Content-type", "application/json; charset=utf-8")
        .header("Authorization", format!("Bearer {}", bot_token))
        .send()
        .await?;

    let body = res.text().await?;

    let parsed: Value = serde_json::from_str(&body)?;

    // TODO: use serde_json to get user id and name
    let members = &parsed["users"]["members"];

    Ok(body)
}
