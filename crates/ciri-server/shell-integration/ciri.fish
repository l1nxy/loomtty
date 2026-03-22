# Ciri shell integration for Fish

if set -q CIRI_SHELL_INTEGRATION
    return
end
set -gx CIRI_SHELL_INTEGRATION 1

function __ciri_prompt --on-event fish_prompt
    set -l exit_code $status
    printf '\e]133;D;%d\e\\' $exit_code
    printf '\e]7;file://%s%s\e\\' (hostname) "$PWD"
    printf '\e]133;A\e\\'
end

function __ciri_preexec --on-event fish_preexec
    printf '\e]133;C\e\\'
end
