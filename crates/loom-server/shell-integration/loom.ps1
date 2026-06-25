# Loom shell integration for PowerShell (Windows PowerShell 5.1 and PowerShell 7+)
#
# Emits OSC 133 prompt marks (jump-to-prompt, command status, exit codes) and
# OSC 7 working-directory reports, mirroring loom.bash / loom.zsh.
#
# Enable by dot-sourcing this file from your PowerShell profile:
#
#   notepad $PROFILE                      # create / edit your profile
#   # then add (adjust the path to your install location):
#   . "$env:LOCALAPPDATA\Programs\loomtty\shell-integration\loom.ps1"
#
# The Windows installer drops this script in <install dir>\shell-integration.

# Only activate in interactive sessions.
if (-not [Environment]::UserInteractive) { return }

# Guard against double-sourcing.
if ($env:LOOM_SHELL_INTEGRATION) { return }
$env:LOOM_SHELL_INTEGRATION = '1'

# ESC as a value so this works on PowerShell 5.1 (no `e escape).
$global:__loom_esc = [char]27

# Wrap a payload in an OSC … ST sequence (ESC ] … ESC \).
function global:__loom_osc([string]$body) {
    return "$($global:__loom_esc)]$body$($global:__loom_esc)\"
}

# Percent-encode a path for an OSC 7 file:// URI, preserving the unreserved
# set and '/' (same rule as the bash/zsh helpers).
function global:__loom_urlencode([string]$s) {
    $sb = [System.Text.StringBuilder]::new()
    foreach ($b in [System.Text.Encoding]::UTF8.GetBytes($s)) {
        $c = [char]$b
        if ($c -match '[-_.~/a-zA-Z0-9]') {
            [void]$sb.Append($c)
        } else {
            [void]$sb.AppendFormat('%{0:X2}', $b)
        }
    }
    return $sb.ToString()
}

# Preserve the prompt the user already had so we can chain into it. Capture the
# scriptblock itself (via $function:prompt), not its source text — round-tripping
# through Get-Content loses the closure / module scope that prompts like
# oh-my-posh and Starship rely on. The double-load guard above runs this once.
$global:__loom_prev_prompt = if (Test-Path Function:\prompt) { $function:prompt } else { $null }

function global:prompt {
    # Capture the previous command's status FIRST — every statement below
    # overwrites $? and most leave $LASTEXITCODE untouched. PowerShell only
    # updates $LASTEXITCODE for native executables, so it goes stale: a cmdlet
    # that succeeds after a failed native command leaves a non-zero value, and a
    # cmdlet that *fails* leaves whatever the last native command set. Derive the
    # exit code from $?, and trust $LASTEXITCODE only when it's non-zero AND
    # changed since the last prompt (i.e. a native command actually ran and
    # failed this cycle); otherwise report a generic 1 for the cmdlet failure.
    $loomOk = $?
    $loomNativeExit = $global:LASTEXITCODE
    if ($loomOk) {
        $loomExit = 0
    } elseif ($loomNativeExit -and $loomNativeExit -ne $global:__loom_prev_exit) {
        $loomExit = $loomNativeExit
    } else {
        $loomExit = 1
    }
    $global:__loom_prev_exit = $loomNativeExit

    # OSC 133;D — previous command finished, with its exit code.
    $out = __loom_osc "133;D;$loomExit"

    # OSC 7 — report the working directory as a file:// URI.
    $cwd = (Get-Location).ProviderPath
    if ($cwd) {
        $enc = __loom_urlencode ($cwd -replace '\\', '/')
        $out += __loom_osc "7;file://$env:COMPUTERNAME/$enc"
    }

    # OSC 133;A — prompt start.
    $out += __loom_osc "133;A"

    # The user's original prompt body (or a sensible default).
    if ($global:__loom_prev_prompt) {
        $out += (& $global:__loom_prev_prompt)
    } else {
        $out += "PS $cwd> "
    }

    # NB: OSC 133;C (command/output start) is NOT emitted here — drawing the
    # prompt is not when the command runs. It is emitted from the
    # PSConsoleHostReadLine wrapper below, after the user submits the line.

    # Restore $LASTEXITCODE so our probing doesn't leak into the next prompt.
    $global:LASTEXITCODE = $loomNativeExit
    return $out
}

# OSC 133;C marks the start of command execution (output start). PowerShell has
# no preexec hook, but when PSReadLine is loaded the host reads every
# interactive command through PSConsoleHostReadLine — wrap it to emit C right
# after the user submits the line and before it runs. This mirrors loom.bash's
# preexec (DEBUG trap); emitting C while drawing the prompt instead would
# wrongly count prompt-idle and typing time as command output. The double-load
# guard at the top keeps this from wrapping itself on re-source. Without
# PSReadLine there is no safe hook, so C is skipped rather than emitted early.
if (Test-Path Function:\PSConsoleHostReadLine) {
    $global:__loom_prev_readline = $function:PSConsoleHostReadLine
    function global:PSConsoleHostReadLine {
        $command = & $global:__loom_prev_readline
        # Skip blank submissions (bare Enter) — no command actually runs.
        if (-not [string]::IsNullOrWhiteSpace($command)) {
            [Console]::Write((__loom_osc "133;C"))
        }
        return $command
    }
}
