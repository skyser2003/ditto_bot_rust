use crate::{slack, Message};
use redis::Commands;
use slack::UsersList;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn handle<'a, B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    let mut conn = bot.redis();

    let _ = increase_chat_count(&mut conn, &msg.user);

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

    if call_type == "잉여" {
        log::debug!("Surplus: bot command full text = {:?}", &msg.text);
        log::debug!("call_type: {:?}", call_type);

        let mut table = std::collections::HashMap::<String, i32>::new();

        let records: Vec<String> = conn.zrange("ditto-archive", 0, -1).unwrap();

        if records.is_empty() {
            return bot
                .send_message(
                    &msg.channel,
                    Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                        text: slack::TextObject {
                            ty: slack::TextObjectType::PlainText,
                            text: "[There is no chat record.]".to_string(),
                            emoji: None,
                            verbatim: None,
                        },
                        block_id: None,
                        fields: None,
                    })]),
                    None,
                    None,
                )
                .await
                .and(Ok(()));
        }

        for record in records {
            let user_id = record.split(':').nth(1).unwrap().to_string();

            let prev_count = table.get(&user_id);
            let next_count = match prev_count {
                Some(val) => val + 1,
                None => 1,
            };

            table.insert(user_id, next_count);
        }

        let mut vec_table = Vec::<(&String, i32)>::new();

        for pair in table.iter_mut() {
            vec_table.push((pair.0, *pair.1));
        }

        vec_table.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        if slices.len() <= 1 || slices[1] != "all" {
            // Top 5 user's chat count only
            vec_table.truncate(5);
        }

        let user_name_map = get_users_list(bot.bot_token())
            .await
            .unwrap_or_else(|_| HashMap::<String, String>::new());

        let largest_count = vec_table.iter().map(|pair| pair.1).max().unwrap_or(0);

        let mut vec_bar = Vec::<String>::new();

        for pair in vec_table {
            let user_name = user_name_map.get(pair.0).unwrap_or(pair.0);
            let user_bar = format!(
                "*`{:}`:*\n\t{:} {:}",
                user_name,
                generate_bar(pair.1, largest_count, 2),
                pair.1
            );

            vec_bar.push(user_bar);
        }

        let graph_text = vec_bar.join("\n");

        return bot
            .send_message(
                &msg.channel,
                Message::Blocks(&[slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::Markdown,
                        text: graph_text,
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                })]),
                None,
                None,
            )
            .await
            .and(Ok(()));
    }

    Ok(())
}

fn increase_chat_count(conn: &mut redis::Connection, user_id: &str) -> anyhow::Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let score = now.as_millis();
    let member = format!("{}:{}", score, user_id);

    conn.zadd("ditto-archive", member, score as i64)?;

    Ok(())
}

fn generate_bar(chat_count: i32, largest_count: i32, level: usize) -> String {
    let characters = ["", "▌", "█"];
    let steps = max(min(level, characters.len() - 1), 1);

    let n = (20.0 * (chat_count as f32 / largest_count as f32)).ceil() as i32;
    let graph_char = characters[steps];

    let length = n / steps as i32;

    let mut graph_str = (0..length).map(|_| graph_char).collect::<String>();

    graph_str.push_str(characters[(n % steps as i32) as usize]);

    graph_str
}

async fn get_users_list(bot_token: &str) -> anyhow::Result<HashMap<String, String>> {
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

    let users_list = serde_json::from_str::<UsersList>(&body)?;
    let members = users_list.members;

    let mut name_map = HashMap::<String, String>::new();

    for member in members {
        name_map.insert(member.id, member.name);
    }

    Ok(name_map)
}
