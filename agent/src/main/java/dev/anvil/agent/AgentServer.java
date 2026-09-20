package dev.anvil.agent;

import org.bukkit.Bukkit;
import org.bukkit.entity.Player;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

final class AgentServer implements AutoCloseable {
    private static final String CODE_CHARS = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

    private final AnvilAgentPlugin plugin;
    private final IdentityStore identity;
    private final String bind;
    private final int port;
    private final long pairCodeTtlSeconds;
    private final AtomicBoolean running = new AtomicBoolean();

    private volatile ServerSocket serverSocket;
    private volatile String pairCode;
    private volatile Instant pairCodeExpiresAt = Instant.EPOCH;

    AgentServer(
        AnvilAgentPlugin plugin,
        IdentityStore identity,
        String bind,
        int port,
        long pairCodeTtlSeconds
    ) {
        this.plugin = plugin;
        this.identity = identity;
        this.bind = bind;
        this.port = port;
        this.pairCodeTtlSeconds = pairCodeTtlSeconds;
    }

    void start() throws IOException {
        ServerSocket socket = new ServerSocket();
        socket.setReuseAddress(true);
        socket.bind(new InetSocketAddress(InetAddress.getByName(bind), port));
        serverSocket = socket;
        running.set(true);

        rotatePairCode();

        Thread.ofPlatform()
            .name("anvil-agent-accept")
            .daemon(true)
            .start(this::acceptLoop);
    }

    synchronized String rotatePairCode() {
        var random = new java.security.SecureRandom();
        StringBuilder code = new StringBuilder(9);
        for (int i = 0; i < 8; i++) {
            if (i == 4) code.append('-');
            code.append(CODE_CHARS.charAt(random.nextInt(CODE_CHARS.length())));
        }
        pairCode = code.toString();
        pairCodeExpiresAt = Instant.now().plusSeconds(pairCodeTtlSeconds);
        return pairCode;
    }

    String bind() {
        return bind;
    }

    int port() {
        return port;
    }

    private void acceptLoop() {
        while (running.get()) {
            try {
                Socket socket = serverSocket.accept();
                Thread.startVirtualThread(() -> handle(socket));
            } catch (SocketException e) {
                if (running.get()) {
                    plugin.getLogger().warning("Anvil accept loop stopped: " + e.getMessage());
                }
                return;
            } catch (IOException e) {
                plugin.getLogger().warning("Anvil accept failed: " + e.getMessage());
            }
        }
    }

    private void handle(Socket socket) {
        try (socket;
             var reader = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8));
             var writer = new BufferedWriter(new OutputStreamWriter(socket.getOutputStream(), StandardCharsets.UTF_8))) {

            socket.setSoTimeout(10_000);
            String line = reader.readLine();
            if (line == null || line.isBlank()) return;
            if (line.length() > 4096) {
                reply(writer, Json.error("request_too_large", "request line exceeds 4096 bytes"));
                return;
            }

            String[] parts = line.trim().split(" ", 4);
            if (parts.length >= 2 && parts[0].equals("PAIR")) {
                handlePair(parts[1], writer);
                return;
            }

            if (parts.length < 3 || !parts[0].equals("AUTH")) {
                reply(writer, Json.error("bad_request", "expected PAIR <code> or AUTH <token> <command>"));
                return;
            }

            String expected = identity.token();
            if (expected == null || !constantTimeEquals(expected, parts[1])) {
                reply(writer, Json.error("unauthorized", "invalid Anvil token"));
                return;
            }

            switch (parts[2]) {
                case "SERVER_INFO" -> reply(writer, serverInfoJson());
                case "PLAYERS" -> reply(writer, playersJson());
                default -> reply(writer, Json.error("unknown_command", "unknown command: " + parts[2]));
            }
        } catch (Exception e) {
            plugin.getLogger().warning("Anvil client error: " + e.getMessage());
        }
    }

    private void handlePair(String code, BufferedWriter writer) throws IOException {
        String current = pairCode;
        if (current == null || Instant.now().isAfter(pairCodeExpiresAt)) {
            reply(writer, Json.error("pair_expired", "pair code expired; run /anvil pair again"));
            return;
        }
        if (!constantTimeEquals(current, code)) {
            reply(writer, Json.error("pair_denied", "invalid pair code"));
            return;
        }

        String token = identity.tokenOrCreate();
        pairCode = null;
        pairCodeExpiresAt = Instant.EPOCH;

        reply(writer,
            "{\"ok\":true,\"type\":\"paired\",\"server_id\":" + Json.quote(identity.serverId())
                + ",\"token\":" + Json.quote(token) + "}");
    }

    private String serverInfoJson() throws Exception {
        return sync(() -> {
            String implementation = Bukkit.getName() + " " + Bukkit.getVersion();
            return "{\"ok\":true,\"type\":\"server_info\",\"server_id\":"
                + Json.quote(identity.serverId())
                + ",\"name\":" + Json.quote(Bukkit.getServer().getName())
                + ",\"minecraft_version\":" + Json.quote(Bukkit.getMinecraftVersion())
                + ",\"implementation\":" + Json.quote(implementation)
                + ",\"online_players\":" + Bukkit.getOnlinePlayers().size()
                + ",\"max_players\":" + Bukkit.getMaxPlayers()
                + "}";
        });
    }

    private String playersJson() throws Exception {
        return sync(() -> {
            List<String> encoded = new ArrayList<>();
            for (Player player : Bukkit.getOnlinePlayers()) {
                encoded.add("{\"name\":" + Json.quote(player.getName())
                    + ",\"uuid\":" + Json.quote(player.getUniqueId().toString())
                    + ",\"ping_ms\":" + player.getPing()
                    + ",\"world\":" + Json.quote(player.getWorld().getName())
                    + ",\"game_mode\":" + Json.quote(player.getGameMode().name().toLowerCase())
                    + "}");
            }
            return "{\"ok\":true,\"type\":\"players\",\"players\":["
                + String.join(",", encoded) + "]}";
        });
    }

    private <T> T sync(java.util.concurrent.Callable<T> task) throws Exception {
        if (Bukkit.isPrimaryThread()) return task.call();
        Future<T> future = Bukkit.getScheduler().callSyncMethod(plugin, task);
        return future.get(3, TimeUnit.SECONDS);
    }

    private static void reply(BufferedWriter writer, String json) throws IOException {
        writer.write(json);
        writer.write('\n');
        writer.flush();
    }

    private static boolean constantTimeEquals(String a, String b) {
        byte[] left = a.getBytes(StandardCharsets.UTF_8);
        byte[] right = b.getBytes(StandardCharsets.UTF_8);
        int diff = left.length ^ right.length;
        int length = Math.max(left.length, right.length);
        for (int i = 0; i < length; i++) {
            byte l = i < left.length ? left[i] : 0;
            byte r = i < right.length ? right[i] : 0;
            diff |= l ^ r;
        }
        return diff == 0;
    }

    @Override
    public void close() {
        running.set(false);
        ServerSocket socket = serverSocket;
        if (socket != null) {
            try {
                socket.close();
            } catch (IOException ignored) {
            }
        }
    }
}
