# Loom shell integration for Zsh

# Only activate in interactive shells
[[ ! -o interactive ]] && return

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
    printf '\e]133;D;%d\e\\' "$exit_code"
    printf '\e]7;file://%s%s\e\\' "$(hostname)" "$(__loom_urlencode "$PWD")"
    printf '\e]133;A\e\\'
}

__loom_preexec() {
    printf '\e]133;C\e\\'
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __loom_precmd
add-zsh-hook preexec __loom_preexec
