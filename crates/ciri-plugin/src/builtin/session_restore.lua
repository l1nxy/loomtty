-- Built-in agent detection for session restore.
-- Users can override by registering their own "detect-agent" handler
-- in ~/.config/ciri/init.lua (last-registered handler wins).

local agents = {
    {
        exe = "claude",
        name = "claude-code",
        resume = "claude --continue",
        -- Non-interactive flags: skip detection if any of these appear.
        skip_args = { "--print", "--chrome-native-host", "--pipe", "-p", "mcp" },
    },
    {
        exe = "codex",
        name = "codex",
        resume = "codex resume --last",
        skip_subcommand = "exec",
    },
    {
        exe = "opencode",
        name = "opencode",
        resume = "opencode --continue",
        skip_subcommand = "run",
    },
    {
        exe = "droid",
        name = "droid",
        resume = "droid",
        skip_subcommand = "exec",
    },
}

local function has_skip_arg(skip_args, argv)
    for _, skip in ipairs(skip_args) do
        for _, arg in ipairs(argv) do
            if arg == skip then
                return true
            end
        end
    end
    return false
end

ciri.on("detect-agent", function(exe_name, argv)
    for _, agent in ipairs(agents) do
        if exe_name == agent.exe then
            -- Check skip_args: if any arg matches, this is non-interactive.
            if agent.skip_args and has_skip_arg(agent.skip_args, argv) then
                return nil
            end
            -- Check skip_subcommand: if argv[2] matches, non-interactive.
            if agent.skip_subcommand and argv[2] == agent.skip_subcommand then
                return nil
            end
            return { name = agent.name, resume_command = agent.resume }
        end
    end
    return nil
end)
