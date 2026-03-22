# Ciri shell integration for Bash
# Injected automatically via PROMPT_COMMAND

# Guard against double-sourcing
[[ -n "$CIRI_SHELL_INTEGRATION" ]] && return
export CIRI_SHELL_INTEGRATION=1

__ciri_precmd() {
    local exit_code=$?
    # Report command completion with exit code (OSC 133;D)
    printf '\e]133;D;%d\e\\' "$exit_code"
    # Report working directory (OSC 7)
    printf '\e]7;file://%s%s\e\\' "$(hostname)" "$PWD"
    # Mark prompt start (OSC 133;A)
    printf '\e]133;A\e\\'
}

__ciri_preexec() {
    # Mark command start (OSC 133;C)
    printf '\e]133;C\e\\'
}

# Install via PROMPT_COMMAND
if [[ -z "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__ciri_precmd"
else
    PROMPT_COMMAND="__ciri_precmd;$PROMPT_COMMAND"
fi

# Bash preexec via DEBUG trap
trap '__ciri_preexec' DEBUG
