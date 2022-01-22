[![Build, deploy, test](https://github.com/skyser2003/ditto_bot_rust/workflows/Rust/badge.svg)](https://github.com/skyser2003/ditto_bot_rust/actions?query=workflow%3ARust)

# Usage

```bash
SLACK_BOT_TOKEN=$() SLACK_SIGNING_SECRET=$() cargo run
```

# Test

```bash
cargo test -- --no-capture
```

`test::MockBot`을 활용해서 메시지 테스트를 하면 slack에 post하는 메시지 데이터를 볼 수 있다.
https://api.slack.com/docs/messages/builder 에서 해당 메시지가 올바른지 테스트 해 볼 수 있다.
