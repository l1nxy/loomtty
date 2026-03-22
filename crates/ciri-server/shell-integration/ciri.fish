# Ciri shell integration for Fish

if set -q CIRI_SHELL_INTEGRATION
    return
end
set -gx CIRI_SHELL_INTEGRATION 1

function __ciri_urlencode
    set -l str $argv[1]
    set -l encoded ""
    for i in (string split '' -- $str)
        switch $i
            case 'a' 'b' 'c' 'd' 'e' 'f' 'g' 'h' 'i' 'j' 'k' 'l' 'm' 'n' 'o' 'p' 'q' 'r' 's' 't' 'u' 'v' 'w' 'x' 'y' 'z' 'A' 'B' 'C' 'D' 'E' 'F' 'G' 'H' 'I' 'J' 'K' 'L' 'M' 'N' 'O' 'P' 'Q' 'R' 'S' 'T' 'U' 'V' 'W' 'X' 'Y' 'Z' '0' '1' '2' '3' '4' '5' '6' '7' '8' '9' '-' '_' '.' '~' '/'
                set encoded "$encoded$i"
            case '*'
                set encoded "$encoded"(printf '%%%02X' "'$i")
        end
    end
    echo -n $encoded
end

function __ciri_prompt --on-event fish_prompt
    set -l exit_code $status
    printf '\e]133;D;%d\e\\' $exit_code
    printf '\e]7;file://%s%s\e\\' (hostname) (__ciri_urlencode "$PWD")
    printf '\e]133;A\e\\'
end

function __ciri_preexec --on-event fish_preexec
    printf '\e]133;C\e\\'
end
