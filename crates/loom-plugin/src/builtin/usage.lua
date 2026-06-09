-- Built-in: gate the usage segment by the focused pane's process.
--
-- Receives a context table from Rust. All `*_utilization` fields are
-- 0..1 fractions (so 0.6 = 60% used) — multiply by 100 yourself when
-- displaying as a percentage. Rust deliberately doesn't pre-scale so
-- Lua plugins can render however they want (progress bar, ratio, …).
--
--   ctx.focused_pane = { id, title, cwd } | nil
--   ctx.usage = {
--     claude = { session_utilization, weekly_utilization,
--                session_resets_at, weekly_resets_at,
--                session_status, weekly_status, org_id, last_error } | nil,
--     codex  = { primary_utilization, secondary_utilization,
--                primary_resets_at, secondary_resets_at,
--                plan, balance, has_credits, unlimited, last_error } | nil,
--   }
--   ctx.session_name, ctx.mode, ctx.pane_count
--
-- Return the string to render, or nil to hide the segment entirely.
-- Users can override this by registering their own "format-usage"
-- handler in ~/.config/loom/init.lua — last-registered wins.

-- Convert a 0..1 fraction to a "NN%" display string.
local function pct(frac)
    if frac == nil then return "--" end
    return string.format("%d%%", math.floor(frac * 100 + 0.5))
end

local function title_matches(title, needle)
    if title == nil or title == "" then return false end
    return string.find(title:lower(), needle, 1, true) ~= nil
end

local function format_claude(claude)
    if claude == nil then return nil end
    if claude.last_error then return "claude !" end
    return string.format(
        "claude %s/%s",
        pct(claude.session_utilization),
        pct(claude.weekly_utilization)
    )
end

local function format_codex(codex)
    if codex == nil then return nil end
    if codex.last_error then return "codex !" end
    return string.format(
        "codex %s/%s",
        pct(codex.primary_utilization),
        pct(codex.secondary_utilization)
    )
end

loom.on("format-usage", function(ctx)
    local fp = ctx.focused_pane
    local usage = ctx.usage or {}
    if fp == nil then return nil end

    -- Prefer the server-detected agent (real foreground-process
    -- probe via procinfo + tcgetpgrp / Windows process tree). Fall
    -- back to the pane title only when the server hasn't reported
    -- an agent yet (e.g. fresh pane, before the first 30s tick).
    local agent = fp.agent
    if agent == "claude-code" then
        return format_claude(usage.claude)
    elseif agent == "codex" then
        return format_codex(usage.codex)
    elseif agent == nil then
        local low = (fp.title or ""):lower()
        if title_matches(low, "claude") then
            return format_claude(usage.claude)
        elseif title_matches(low, "codex") then
            return format_codex(usage.codex)
        end
    end

    -- Focused pane is something else (vim, shell, …) — hide the
    -- segment entirely.
    return nil
end)
