package dev.anvil.agent;

final class Json {
    private Json() {}

    static String quote(String value) {
        if (value == null) return "null";
        StringBuilder out = new StringBuilder(value.length() + 16);
        out.append('"');
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '"' -> out.append("\\"");
                case '\\' -> out.append("\\\\");
                case '\b' -> out.append("\\b");
                case '\f' -> out.append("\\f");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }

    static String error(String code, String message) {
        return "{\"ok\":false,\"type\":\"error\",\"code\":"
            + quote(code) + ",\"message\":" + quote(message) + "}";
    }
}
