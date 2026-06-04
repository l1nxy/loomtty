# Loom shell integration for Bash
# Injected automatically via PROMPT_COMMAND

# Only activate in interactive shells (BASH_ENV is sourced by non-interactive bash too)
[[ $- != *i* ]] && return

# Guard against double-sourcing
[[ -n "$LOOM_SHELL_INTEGRATION" ]] && return
export LOOM_SHELL_INTEGRATION=1

__loom_urlencode() {
    local string="$1"
    local strlen=${#string}
    local encoded=""
    local pos c o
    for (( pos=0 ; pos<strlen ; pos++ )); do
        c=${string:$pos:1}
        case "$c" in
            [-_.~/a-zA-Z0-9]) encoded+="$c" ;;
            *) printf -v o '%%%02X' "'$c"; encoded+="$o" ;;
        esac
    done
    printf '%s' "$encoded"
}

__loom_precmd() {
    local exit_code=$?
    # Report command completion with exit code (OSC 133;D)
    printf '\e]133;D;%d\e\\' "$exit_code"
    # Report working directory (OSC 7)
    printf '\e]7;file://%s%s\e\\' "$(hostname)" "$(__loom_urlencode "$PWD")"
    # Mark prompt start (OSC 133;A)
    printf '\e]133;A\e\\'
}

__loom_preexec() {
    # Mark command start (OSC 133;C)
    printf '\e]133;C\e\\'
}

# Install via PROMPT_COMMAND
if [[ -z "$PROMPT_COMMAND" ]]; then
    PROMPT_COMMAND="__loom_precmd"
else
    PROMPT_COMMAND="__loom_precmd;$PROMPT_COMMAND"
fi

# Bash preexec via DEBUG trap
trap '__loom_preexec' DEBUG
