// This file was created by Claude Opus 5.5

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.WebSocket;
import java.time.Duration;
import java.time.Instant;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.TimeoutException;

/**
 * eCall / notification mock for the Guardian Loop.
 *
 * Listens to the Guardian's EWS WebSocket (one JSON state update per second)
 * and sends a Telegram message whenever the Guardian state changes.
 *
 * Environment:
 *   GUARDIAN_EWS_URL    ws://localhost:8765/ws by default
 *   TELEGRAM_BOT_TOKEN  bot token from @BotFather (unset: dry-run, log only)
 *   TELEGRAM_CHAT_ID    target chat id (unset: dry-run, log only)
 *   RECONNECT_DELAY_S   delay between reconnect attempts, 3 by default
 */
public final class NotificationService {

    /** Guardian broadcasts every second; silence this long means the link is dead. */
    private static final Duration STALE_TIMEOUT = Duration.ofSeconds(10);

    private final URI ewsUri;
    private final Duration reconnectDelay;
    private final HttpClient http = HttpClient.newBuilder()
            .connectTimeout(Duration.ofSeconds(5))
            .build();
    private final TelegramNotifier notifier;
    private final ExecutorService sender = Executors.newSingleThreadExecutor();

    private volatile boolean running = true;
    private volatile WebSocket socket;
    private volatile long lastFrameNanos;

    NotificationService(URI ewsUri, Duration reconnectDelay, String botToken, String chatId) {
        this.ewsUri = ewsUri;
        this.reconnectDelay = reconnectDelay;
        this.notifier = new TelegramNotifier(http, botToken, chatId);
    }

    public static void main(String[] args) throws InterruptedException {
        NotificationService service = new NotificationService(
                URI.create(env("GUARDIAN_EWS_URL", "ws://localhost:8765/ws")),
                Duration.ofSeconds(Long.parseLong(env("RECONNECT_DELAY_S", "3"))),
                System.getenv("TELEGRAM_BOT_TOKEN"),
                System.getenv("TELEGRAM_CHAT_ID"));

        Runtime.getRuntime().addShutdownHook(new Thread(service::shutdown, "shutdown"));
        service.run();
    }

    void run() throws InterruptedException {
        log("Notification service starting, Guardian EWS at " + ewsUri
                + (notifier.isDryRun() ? " (dry-run: TELEGRAM_BOT_TOKEN/TELEGRAM_CHAT_ID not set)" : ""));

        while (running) {
            CompletableFuture<Void> closed = new CompletableFuture<>();
            try {
                socket = http.newWebSocketBuilder()
                        .connectTimeout(Duration.ofSeconds(5))
                        .buildAsync(ewsUri, new EwsListener(closed))
                        .join();
                lastFrameNanos = System.nanoTime();
                log("Connected to Guardian EWS");
                awaitClosedOrStale(closed);
                log("Disconnected from Guardian EWS");
            } catch (CompletionException e) {
                Throwable cause = e.getCause() != null ? e.getCause() : e;
                log("Cannot connect to " + ewsUri + ": " + cause);
            }

            if (running) {
                Thread.sleep(reconnectDelay.toMillis());
            }
        }
    }

    private void awaitClosedOrStale(CompletableFuture<Void> closed) throws InterruptedException {
        while (running) {
            try {
                closed.get(1, TimeUnit.SECONDS);
                return;
            } catch (TimeoutException e) {
                if (System.nanoTime() - lastFrameNanos > STALE_TIMEOUT.toNanos()) {
                    log("No EWS update for " + STALE_TIMEOUT.toSeconds() + " s, dropping connection");
                    socket.abort();
                    return;
                }
            } catch (ExecutionException e) {
                return;
            }
        }
    }

    private void shutdown() {
        running = false;
        WebSocket ws = socket;
        if (ws != null) {
            ws.abort();
        }
        sender.shutdown();
        try {
            // Let an in-flight Telegram message go out before the JVM exits.
            sender.awaitTermination(5, TimeUnit.SECONDS);
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        log("Notification service stopped");
    }

    /** Receives EWS frames for one connection; the first frame announces the current state. */
    private final class EwsListener implements WebSocket.Listener {
        private final CompletableFuture<Void> closed;
        private final StringBuilder buffer = new StringBuilder();
        private EwsState previous;

        EwsListener(CompletableFuture<Void> closed) {
            this.closed = closed;
        }

        @Override
        public void onOpen(WebSocket webSocket) {
            webSocket.request(1);
        }

        @Override
        public CompletionStage<?> onText(WebSocket webSocket, CharSequence data, boolean last) {
            buffer.append(data);
            if (last) {
                lastFrameNanos = System.nanoTime();
                handleFrame(buffer.toString());
                buffer.setLength(0);
            }
            webSocket.request(1);
            return null;
        }

        @Override
        public CompletionStage<?> onClose(WebSocket webSocket, int statusCode, String reason) {
            closed.complete(null);
            return null;
        }

        @Override
        public void onError(WebSocket webSocket, Throwable error) {
            log("EWS connection error: " + error);
            closed.complete(null);
        }

        private void handleFrame(String json) {
            EwsState current;
            try {
                current = EwsState.parse(json);
            } catch (IllegalArgumentException e) {
                log("Skipping invalid EWS frame: " + e.getMessage());
                return;
            }

            if (previous == null) {
                log("Guardian state on connect: " + current.state());
                notifyAsync(TelegramNotifier.formatConnected(current));
            } else if (!previous.state().equals(current.state())) {
                log("Guardian state changed: " + previous.state() + " -> " + current.state());
                notifyAsync(TelegramNotifier.formatTransition(previous, current));
            }
            previous = current;
        }
    }

    private void notifyAsync(String text) {
        if (!sender.isShutdown()) {
            sender.execute(() -> notifier.send(text));
        }
    }

    private static String env(String name, String fallback) {
        String value = System.getenv(name);
        return value == null || value.isBlank() ? fallback : value;
    }

    static void log(String message) {
        System.out.println(Instant.now() + " " + message);
    }
}
