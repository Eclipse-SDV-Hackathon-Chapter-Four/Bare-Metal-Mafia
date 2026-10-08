# Notification service and EWS API

The Guardian exposes an Early Warning System (EWS) API, and this service uses it
to send a Telegram message on every Guardian state change.

## EWS API

The Guardian serves the API as a WebSocket at `ws://localhost:8765/ws`.
It sends a JSON `GuardianState` update every second. The `time` value is Unix
time in milliseconds, and `state` is the Guardian's current state enum. EWS
warnings are received as JSON `EWSWarn` messages on the same connection.

Guardian state, sent every second:

```Rust
struct EwsGuardianState {
    time: u64,

    temperature: f32,
    child_presence: bool,
    state: GuardianState,

    hvac_active: bool,
    hvac_target: f32,
    hvac_fault: bool,
};

enum GuardianState {
    Clear,
    Monitoring,
    Warning,
    Critical,
    Mitigating,
}
```

Warning, sent to the Guardian:

```Rust
struct EWSWarn {
    time: u64,
    reason: Reason,
}

enum Reason {
    Reset,
    Heat,
}
```

## Telegram notifications

*(Contents of this section were created by Claude Opus 5.5)*

`notification/` is a dependency-free Java service that connects to the EWS WebSocket
and sends a Telegram message on every Guardian state change. It reconnects if the
Guardian restarts.

1. Create a bot with [@BotFather](https://t.me/BotFather) and copy the token.
2. Send the bot a message, then read your chat id from
   `https://api.telegram.org/bot<token>/getUpdates` (`message.chat.id`).
3. Put both into `.env` (git-ignored) or export them:

   ```bash
   TELEGRAM_BOT_TOKEN=123456:ABC...
   TELEGRAM_CHAT_ID=987654321
   ```

4. `docker compose up --build` starts the service next to the Guardian.

If either variable is unset, the service runs in dry-run mode and only logs the messages
(`docker compose logs -f notification`). To run it outside Docker, from the repository root:

```bash
javac -d notification/out notification/src/*.java
GUARDIAN_EWS_URL=ws://localhost:8765/ws java -cp notification/out NotificationService
```
