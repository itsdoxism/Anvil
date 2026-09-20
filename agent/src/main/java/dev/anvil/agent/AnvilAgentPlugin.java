package dev.anvil.agent;

import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.plugin.java.JavaPlugin;

import java.io.IOException;
import java.nio.file.Path;

public final class AnvilAgentPlugin extends JavaPlugin {
    private AgentServer agent;

    @Override
    public void onEnable() {
        saveDefaultConfig();

        try {
            IdentityStore identity = IdentityStore.load(getDataFolder().toPath());
            String bind = getConfig().getString("bind", "127.0.0.1");
            int port = getConfig().getInt("port", 45920);
            long ttl = getConfig().getLong("pair-code-ttl-seconds", 600);
            long maxTransferBytes = getConfig().getLong("max-transfer-bytes", 134_217_728L);
            Path serverRoot = Path.of(".").toAbsolutePath().normalize();

            agent = new AgentServer(
                this,
                identity,
                bind,
                port,
                ttl,
                maxTransferBytes,
                serverRoot
            );
            agent.start();

            getLogger().info("Anvil Agent listening on " + bind + ":" + port);
            getLogger().info("Filesystem root: " + agent.serverRoot());
            if (!isLoopback(bind)) {
                getLogger().warning("The v0 Anvil transport is not encrypted yet. Do not expose this port to the public Internet.");
            }
            getLogger().info("Pair code: " + agent.rotatePairCode());
        } catch (IOException e) {
            getLogger().severe("Failed to start Anvil Agent: " + e.getMessage());
            getServer().getPluginManager().disablePlugin(this);
        }
    }

    @Override
    public void onDisable() {
        if (agent != null) {
            agent.close();
            agent = null;
        }
    }

    @Override
    public boolean onCommand(CommandSender sender, Command command, String label, String[] args) {
        if (!command.getName().equalsIgnoreCase("anvil")) return false;
        if (!sender.hasPermission("anvil.admin")) {
            sender.sendMessage("You do not have permission to manage Anvil Agent.");
            return true;
        }

        if (args.length == 1 && args[0].equalsIgnoreCase("pair")) {
            if (agent == null) {
                sender.sendMessage("Anvil Agent is not running.");
            } else {
                sender.sendMessage("Anvil pair code: " + agent.rotatePairCode());
                sender.sendMessage("Endpoint: " + agent.bind() + ":" + agent.port());
            }
            return true;
        }

        sender.sendMessage("Usage: /anvil pair");
        return true;
    }

    private static boolean isLoopback(String bind) {
        return bind.equals("127.0.0.1") || bind.equals("::1") || bind.equalsIgnoreCase("localhost");
    }
}
