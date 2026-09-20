package dev.anvil.agent;

import org.bukkit.Bukkit;
import org.bukkit.entity.Player;

import java.io.*;
import java.net.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.security.MessageDigest;
import java.time.Instant;
import java.util.*;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;

final class AgentServer implements AutoCloseable {
    private static final String CODE_CHARS = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    private static final int MAX_HEADER_BYTES = 4096;

    private final AnvilAgentPlugin plugin;
    private final IdentityStore identity;
    private final String bind;
    private final int port;
    private final long pairCodeTtlSeconds;
    private final long maxTransferBytes;
    private final Path serverRoot;
    private final AtomicBoolean running = new AtomicBoolean();

    private volatile ServerSocket serverSocket;
    private volatile String pairCode;
    private volatile Instant pairCodeExpiresAt = Instant.EPOCH;

    AgentServer(
        AnvilAgentPlugin plugin,
        IdentityStore identity,
        String bind,
        int port,
        long pairCodeTtlSeconds,
        long maxTransferBytes,
        Path serverRoot
    ) {
        this.plugin = plugin;
        this.identity = identity;
        this.bind = bind;
        this.port = port;
        this.pairCodeTtlSeconds = pairCodeTtlSeconds;
        this.maxTransferBytes = maxTransferBytes;
        this.serverRoot = serverRoot.toAbsolutePath().normalize();
    }

    void start() throws IOException {
        ServerSocket socket = new ServerSocket();
        socket.setReuseAddress(true);
        socket.bind(new InetSocketAddress(InetAddress.getByName(bind), port));
        serverSocket = socket;
        running.set(true);

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

    String bind() { return bind; }
    int port() { return port; }
    Path serverRoot() { return serverRoot; }

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
             var input = new BufferedInputStream(socket.getInputStream());
             var output = new BufferedOutputStream(socket.getOutputStream())) {

            socket.setSoTimeout(30_000);
            String line = readHeaderLine(input);
            if (line == null || line.isBlank()) return;

            String[] parts = line.trim().split(" ");
            if (parts.length >= 2 && parts[0].equals("PAIR")) {
                handlePair(parts[1], output);
                return;
            }

            if (parts.length < 3 || !parts[0].equals("AUTH")) {
                reply(output, Json.error("bad_request", "expected PAIR <code> or AUTH <token> <command>"));
                return;
            }

            String expected = identity.token();
            if (expected == null || !constantTimeEquals(expected, parts[1])) {
                reply(output, Json.error("unauthorized", "invalid Anvil token"));
                return;
            }

            switch (parts[2]) {
                case "SERVER_INFO" -> reply(output, serverInfoJson());
                case "PLAYERS" -> reply(output, playersJson());
                case "LIST" -> {
                    requireArgs(parts, 4);
                    reply(output, listJson(decodePath(parts[3])));
                }
                case "READ" -> {
                    requireArgs(parts, 4);
                    handleRead(decodePath(parts[3]), output);
                }
                case "WRITE" -> {
                    requireArgs(parts, 6);
                    long size = parseSize(parts[4]);
                    handleWrite(decodePath(parts[3]), size, parts[5], input, output);
                }
                default -> reply(output, Json.error("unknown_command", "unknown command: " + parts[2]));
            }
        } catch (ProtocolException e) {
            plugin.getLogger().fine("Anvil request rejected: " + e.getMessage());
        } catch (Exception e) {
            plugin.getLogger().warning("Anvil client error: " + e.getMessage());
        }
    }

    private void handlePair(String code, OutputStream output) throws IOException {
        String current = pairCode;
        if (current == null || Instant.now().isAfter(pairCodeExpiresAt)) {
            reply(output, Json.error("pair_expired", "pair code expired; run /anvil pair again"));
            return;
        }
        if (!constantTimeEquals(current, code)) {
            reply(output, Json.error("pair_denied", "invalid pair code"));
            return;
        }

        String token = identity.tokenOrCreate();
        pairCode = null;
        pairCodeExpiresAt = Instant.EPOCH;

        reply(output,
            "{\"ok\":true,\"type\":\"paired\",\"server_id\":" + Json.quote(identity.serverId())
                + ",\"token\":" + Json.quote(token) + "}");
    }

    private String listJson(String relative) throws IOException, ProtocolException {
        Path directory;
        try {
            directory = resolveExisting(relative);
        } catch (NoSuchFileException e) {
            return Json.error("not_found", "directory not found: " + relative);
        }
        if (!Files.isDirectory(directory, LinkOption.NOFOLLOW_LINKS)) {
            return Json.error("not_directory", "not a directory: " + relative);
        }

        List<String> encoded = new ArrayList<>();
        try (var stream = Files.list(directory)) {
            stream.sorted(Comparator.comparing(path -> path.getFileName().toString().toLowerCase(Locale.ROOT)))
                .forEach(path -> {
                    try {
                        String childRelative = serverRoot.relativize(path.toAbsolutePath().normalize())
                            .toString().replace(File.separatorChar, '/');
                        String kind;
                        Long size = null;
                        if (Files.isSymbolicLink(path)) {
                            kind = "symlink";
                        } else if (Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS)) {
                            kind = "directory";
                        } else {
                            kind = "file";
                            size = Files.size(path);
                        }

                        encoded.add("{\"name\":" + Json.quote(path.getFileName().toString())
                            + ",\"path\":" + Json.quote(childRelative)
                            + ",\"kind\":" + Json.quote(kind)
                            + ",\"size\":" + (size == null ? "null" : size)
                            + "}");
                    } catch (IOException e) {
                        throw new UncheckedIOException(e);
                    }
                });
        } catch (UncheckedIOException e) {
            throw e.getCause();
        }

        return "{\"ok\":true,\"type\":\"directory\",\"path\":" + Json.quote(relative)
            + ",\"entries\":[" + String.join(",", encoded) + "]}";
    }

    private void handleRead(String relative, OutputStream output) throws IOException, ProtocolException {
        Path file;
        try {
            file = resolveExisting(relative);
        } catch (NoSuchFileException e) {
            reply(output, Json.error("not_found", "file not found: " + relative));
            return;
        }

        if (!Files.isRegularFile(file, LinkOption.NOFOLLOW_LINKS)) {
            reply(output, Json.error("not_file", "not a regular file: " + relative));
            return;
        }

        long size = Files.size(file);
        if (size > maxTransferBytes) {
            reply(output, Json.error("too_large", "file exceeds max-transfer-bytes"));
            return;
        }

        String sha256 = sha256(file);
        reply(output, "{\"ok\":true,\"type\":\"file\",\"path\":" + Json.quote(relative)
            + ",\"size\":" + size + ",\"sha256\":" + Json.quote(sha256) + "}", false);

        try (InputStream fileInput = Files.newInputStream(file)) {
            fileInput.transferTo(output);
        }
        output.flush();
    }

    private void handleWrite(
        String relative,
        long size,
        String expectedSha256,
        InputStream input,
        OutputStream output
    ) throws IOException, ProtocolException {
        if (size < 0 || size > maxTransferBytes) {
            reply(output, Json.error("too_large", "write exceeds max-transfer-bytes"));
            return;
        }
        if (!expectedSha256.matches("[0-9a-fA-F]{64}")) {
            reply(output, Json.error("bad_hash", "sha256 must be 64 hexadecimal characters"));
            return;
        }

        Path target;
        try {
            target = resolveWriteTarget(relative);
        } catch (NoSuchFileException e) {
            reply(output, Json.error("missing_parent", "target parent directory does not exist"));
            return;
        }

        Path parent = target.getParent();
        if (parent == null || !Files.isDirectory(parent, LinkOption.NOFOLLOW_LINKS)) {
            reply(output, Json.error("missing_parent", "target parent directory does not exist"));
            return;
        }

        Path temp = Files.createTempFile(parent, ".anvil-", ".upload");
        MessageDigest digest = sha256Digest();
        long remaining = size;

        try (OutputStream fileOutput = Files.newOutputStream(temp, StandardOpenOption.TRUNCATE_EXISTING)) {
            byte[] buffer = new byte[64 * 1024];
            while (remaining > 0) {
                int want = (int) Math.min(buffer.length, remaining);
                int read = input.read(buffer, 0, want);
                if (read < 0) {
                    throw new EOFException("upload ended before declared size");
                }
                digest.update(buffer, 0, read);
                fileOutput.write(buffer, 0, read);
                remaining -= read;
            }
        } catch (Exception e) {
            Files.deleteIfExists(temp);
            throw e;
        }

        String actualSha256 = HexFormat.of().formatHex(digest.digest());
        if (!actualSha256.equalsIgnoreCase(expectedSha256)) {
            Files.deleteIfExists(temp);
            reply(output, Json.error("checksum_mismatch", "uploaded bytes do not match declared sha256"));
            return;
        }

        try {
            Files.move(temp, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
        } catch (AtomicMoveNotSupportedException e) {
            Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
        }

        reply(output, "{\"ok\":true,\"type\":\"written\",\"path\":" + Json.quote(relative)
            + ",\"size\":" + size + ",\"sha256\":" + Json.quote(actualSha256) + "}");
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
                    + ",\"game_mode\":" + Json.quote(player.getGameMode().name().toLowerCase(Locale.ROOT))
                    + "}");
            }
            return "{\"ok\":true,\"type\":\"players\",\"players\":["
                + String.join(",", encoded) + "]}";
        });
    }

    private Path resolveExisting(String relative) throws IOException, ProtocolException {
        Path candidate = resolveLexical(relative);
        rejectSymlinkComponents(candidate);
        return candidate.toRealPath(LinkOption.NOFOLLOW_LINKS);
    }

    private Path resolveWriteTarget(String relative) throws IOException, ProtocolException {
        Path candidate = resolveLexical(relative);
        Path parent = candidate.getParent();
        if (parent == null) throw new ProtocolException("target has no parent");
        rejectSymlinkComponents(parent);
        Path realParent = parent.toRealPath(LinkOption.NOFOLLOW_LINKS);
        if (!realParent.startsWith(serverRoot.toRealPath())) {
            throw new ProtocolException("path escapes server root");
        }
        return realParent.resolve(candidate.getFileName()).normalize();
    }

    private Path resolveLexical(String relative) throws ProtocolException {
        if (relative.indexOf('\0') >= 0) throw new ProtocolException("path contains NUL");
        Path supplied = Path.of(relative.isBlank() ? "." : relative);
        if (supplied.isAbsolute()) throw new ProtocolException("absolute paths are not allowed");

        Path candidate = serverRoot.resolve(supplied).normalize();
        if (!candidate.startsWith(serverRoot)) {
            throw new ProtocolException("path escapes server root");
        }
        return candidate;
    }

    private void rejectSymlinkComponents(Path candidate) throws IOException, ProtocolException {
        Path current = serverRoot;
        Path relative = serverRoot.relativize(candidate);
        for (Path part : relative) {
            current = current.resolve(part);
            if (Files.exists(current, LinkOption.NOFOLLOW_LINKS) && Files.isSymbolicLink(current)) {
                throw new ProtocolException("symlink paths are not allowed");
            }
        }
    }

    private static String decodePath(String value) throws ProtocolException {
        try {
            return new String(Base64.getUrlDecoder().decode(value), StandardCharsets.UTF_8);
        } catch (IllegalArgumentException e) {
            throw new ProtocolException("invalid encoded path");
        }
    }

    private long parseSize(String value) throws ProtocolException {
        try {
            long parsed = Long.parseLong(value);
            if (parsed < 0) throw new NumberFormatException();
            return parsed;
        } catch (NumberFormatException e) {
            throw new ProtocolException("invalid transfer size");
        }
    }

    private static void requireArgs(String[] parts, int expected) throws ProtocolException {
        if (parts.length != expected) {
            throw new ProtocolException("invalid command argument count");
        }
    }

    private static String readHeaderLine(InputStream input) throws IOException, ProtocolException {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        for (int i = 0; i <= MAX_HEADER_BYTES; i++) {
            int value = input.read();
            if (value < 0) {
                return bytes.size() == 0 ? null : bytes.toString(StandardCharsets.UTF_8);
            }
            if (value == '\n') {
                return bytes.toString(StandardCharsets.UTF_8);
            }
            if (value != '\r') bytes.write(value);
        }
        throw new ProtocolException("request header exceeds 4096 bytes");
    }

    private <T> T sync(java.util.concurrent.Callable<T> task) throws Exception {
        if (Bukkit.isPrimaryThread()) return task.call();
        Future<T> future = Bukkit.getScheduler().callSyncMethod(plugin, task);
        return future.get(3, TimeUnit.SECONDS);
    }

    private static String sha256(Path path) throws IOException {
        MessageDigest digest = sha256Digest();
        try (InputStream input = Files.newInputStream(path)) {
            byte[] buffer = new byte[64 * 1024];
            int read;
            while ((read = input.read(buffer)) >= 0) {
                if (read > 0) digest.update(buffer, 0, read);
            }
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static MessageDigest sha256Digest() {
        try {
            return MessageDigest.getInstance("SHA-256");
        } catch (java.security.NoSuchAlgorithmException e) {
            throw new IllegalStateException("SHA-256 unavailable", e);
        }
    }

    private static void reply(OutputStream output, String json) throws IOException {
        reply(output, json, true);
    }

    private static void reply(OutputStream output, String json, boolean flush) throws IOException {
        output.write(json.getBytes(StandardCharsets.UTF_8));
        output.write('\n');
        if (flush) output.flush();
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

    private static final class ProtocolException extends Exception {
        ProtocolException(String message) {
            super(message);
        }
    }
}
