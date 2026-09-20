package dev.anvil.agent;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.attribute.PosixFilePermissions;
import java.security.SecureRandom;
import java.util.Base64;
import java.util.Properties;
import java.util.UUID;

final class IdentityStore {
    private static final SecureRandom RANDOM = new SecureRandom();

    private final Path path;
    private final String serverId;
    private String token;

    private IdentityStore(Path path, String serverId, String token) {
        this.path = path;
        this.serverId = serverId;
        this.token = token;
    }

    static IdentityStore load(Path dataDirectory) throws IOException {
        Files.createDirectories(dataDirectory);
        Path path = dataDirectory.resolve("identity.properties");
        if (!Files.exists(path)) {
            IdentityStore created = new IdentityStore(path, UUID.randomUUID().toString(), null);
            created.save();
            return created;
        }

        Properties properties = new Properties();
        try (var reader = Files.newBufferedReader(path, StandardCharsets.UTF_8)) {
            properties.load(reader);
        }

        String serverId = properties.getProperty("server-id");
        if (serverId == null || serverId.isBlank()) {
            serverId = UUID.randomUUID().toString();
        }

        String token = properties.getProperty("auth-token");
        if (token != null && token.isBlank()) token = null;

        IdentityStore store = new IdentityStore(path, serverId, token);
        store.save();
        return store;
    }

    synchronized String serverId() {
        return serverId;
    }

    synchronized String token() {
        return token;
    }

    synchronized String tokenOrCreate() throws IOException {
        if (token == null) {
            byte[] bytes = new byte[32];
            RANDOM.nextBytes(bytes);
            token = Base64.getUrlEncoder().withoutPadding().encodeToString(bytes);
            save();
        }
        return token;
    }

    private synchronized void save() throws IOException {
        Properties properties = new Properties();
        properties.setProperty("server-id", serverId);
        if (token != null) properties.setProperty("auth-token", token);

        try (var writer = Files.newBufferedWriter(path, StandardCharsets.UTF_8)) {
            properties.store(writer, "Anvil Agent identity - keep private");
        }

        try {
            Files.setPosixFilePermissions(path, PosixFilePermissions.fromString("rw-------"));
        } catch (UnsupportedOperationException ignored) {
            // Anvil Agent is Linux-first, but failing to set POSIX bits should not corrupt identity.
        }
    }
}
