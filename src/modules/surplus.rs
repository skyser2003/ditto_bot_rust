use crate::MessageEvent;
use redis::{Client, Commands};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn handle<'a>(msg: &'a MessageEvent, redis_client: &Client) -> redis::RedisResult<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let score = now.as_millis();
    let member = format!("{}:{}", score, msg.user);

    let mut conn = redis_client.get_connection().unwrap();
    conn.zadd("ditto-archive", member, score as i64)?;

    Ok(())
}
