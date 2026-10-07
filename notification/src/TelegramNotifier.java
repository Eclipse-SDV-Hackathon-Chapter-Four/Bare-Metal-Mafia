// This file was created by Claude Opus 5.5

import java.io.IOException;
import java.net.URI;
import java.net.URLEncoder;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.time.Instant;
import java.time.ZoneId;
import java.time.format.DateTimeFormatter;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * Sends plain-text messages through the Telegram Bot API (sendMessage).
 * Without a bot token or chat id it runs in dry-run mode and only logs.
 */
final class TelegramNotifier {

    private static final Pattern RETRY_AFTER = Pattern.compile("\"retry_after\"\\s*:\\s*(\\d+)");
    private static final DateTimeFormatter CLOCK =
            DateTimeFormatter.ofPattern("HH:mm:ss").withZone(ZoneId.systemDefault());

    private final HttpClient http;
    private final String token;
    private final String chatId;

    TelegramNotifier(HttpClient http, String token, String chatId) {
        this.http = http;
        this.token = token;
        this.chatId = chatId;
    }

    boolean isDryRun() {
        return token == null || token.isBlank() || chatId == null || chatId.isBlank();
    }

    /** Sends the message; failures are logged, never thrown. */
    void send(String text) {
        if (isDryRun()) {
            NotificationService.log("[dry-run] Telegram message:\n" + text);
            return;
        }

        for (int attempt = 1; attempt <= 2; attempt++) {
            try {
                HttpResponse<String> response = http.send(buildRequest(text), HttpResponse.BodyHandlers.ofString());
                int status = response.statusCode();
                if (status == 200) {
                    NotificationService.log("Telegram message sent: " + text.lines().findFirst().orElse(""));
                    return;
                }
                if (status == 429 && attempt == 1) {
                    long waitSeconds = retryAfterSeconds(response.body());
                    NotificationService.log("Telegram rate limit hit, retrying in " + waitSeconds + " s");
                    Thread.sleep(waitSeconds * 1000);
                    continue;
                }
                NotificationService.log("Telegram API returned " + status + ": " + response.body());
                return;
            } catch (IOException e) {
                NotificationService.log("Telegram request failed: " + e);
                return;
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                return;
            }
        }
    }

    private HttpRequest buildRequest(String text) {
        String form = "chat_id=" + URLEncoder.encode(chatId, StandardCharsets.UTF_8)
                + "&text=" + URLEncoder.encode(text, StandardCharsets.UTF_8);
        return HttpRequest.newBuilder(URI.create("https://api.telegram.org/bot" + token + "/sendMessage"))
                .timeout(Duration.ofSeconds(10))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .POST(HttpRequest.BodyPublishers.ofString(form))
                .build();
    }

    private static long retryAfterSeconds(String body) {
        Matcher m = RETRY_AFTER.matcher(body);
        return m.find() ? Long.parseLong(m.group(1)) : 1;
    }

    static String formatConnected(EwsState current) {
        return "📡 Guardian notifications online\n"
                + headline(current) + "\n"
                + details(current);
    }

    static String formatTransition(EwsState previous, EwsState current) {
        return headline(current) + "   (was " + previous.state() + ")\n"
                + details(current);
    }

    private static String headline(EwsState s) {
        return emoji(s.state()) + " Guardian: " + s.state();
    }

    private static String details(EwsState s) {
        return "Child present: " + (s.childPresence() ? "yes" : "no") + "\n"
                + String.format(Locale.ROOT, "Cabin temperature: %.1f °C%n", s.temperature())
                + "HVAC: " + (s.hvacActive() ? "on" : "off")
                + " · Windows: " + (s.windowsDown() ? "down" : "up") + "\n"
                + CLOCK.format(Instant.ofEpochMilli(s.time()));
    }

    private static String emoji(String state) {
        return switch (state) {
            case "CLEAR" -> "✅";
            case "MONITORING" -> "👀";
            case "WARNING" -> "🟠";
            case "CRITICAL" -> "🔴";
            case "MITIGATING" -> "🛠️";
            default -> "ℹ️";
        };
    }
}
