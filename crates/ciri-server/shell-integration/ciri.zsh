# Ciri shell integration for Zsh

[[ -n "$CIRI_SHELL_INTEGRATION" ]] && return
export CIRI_SHELL_INTEGRATION=1

__ciri_precmd() {
    local exit_code=$?
    printf '\e]133;D;%d\e\\' "$exit_code"
    printf '\e]7;file://%s%s\e\\' "$(hostname)" "$PWD"
    printf '\e]133;A\e\\'
}

__ciri_preexec() {
    printf '\e]133;C\e\\'
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd __ciri_precmd
add-zsh-hook preexec __ciri_preexec
