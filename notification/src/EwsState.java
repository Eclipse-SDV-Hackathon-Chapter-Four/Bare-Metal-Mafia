// This file was created by Claude Opus 5.5

import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * One Guardian state update as broadcast on the EWS WebSocket
 * (see EwsGuardianState in services/src/bin/guardian.rs).
 */
record EwsState(
        long time,
        double temperature,
        boolean childPresence,
        boolean hvacActive,
        boolean windowsDown,
        String state) {

    private static final String NUMBER = "(-?[0-9][0-9.eE+-]*)";
    private static final String BOOL = "(true|false)";
    private static final String STRING = "\"([^\"]*)\"";

    /**
     * Parses the flat EWS JSON object. The payload has a fixed shape with no
     * nesting, so a per-key match is enough and keeps the service dependency-free.
     */
    static EwsState parse(String json) {
        return new EwsState(
                Long.parseLong(field(json, "time", NUMBER)),
                Double.parseDouble(field(json, "temperature", NUMBER)),
                Boolean.parseBoolean(field(json, "child_presence", BOOL)),
                Boolean.parseBoolean(field(json, "hvac_active", BOOL)),
                Boolean.parseBoolean(field(json, "windows_down", BOOL)),
                field(json, "state", STRING));
    }

    private static String field(String json, String key, String valuePattern) {
        Matcher m = Pattern.compile("\"" + key + "\"\\s*:\\s*" + valuePattern).matcher(json);
        if (!m.find()) {
            throw new IllegalArgumentException("missing or malformed field '" + key + "' in " + json);
        }
        return m.group(1);
    }
}
