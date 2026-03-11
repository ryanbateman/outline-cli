# completions.tape — Zsh tab completion demo
#
# Demonstrates: resource-level completions, action-level completions,
# and partial command completion

Output docs/demo/completions.gif

Require outline

Set Shell "zsh"
Set FontSize 16
Set Width 1000
Set Height 500
Set Theme "Catppuccin Mocha"
Set BorderRadius 8
Set Padding 20
Set Margin 20
Set MarginFill "#1e1e2e"
Set TypingSpeed 75ms
Set CursorBlink false

# Setup: install completions for this zsh session
Hide
Type "mkdir -p ~/.zfunc && outline completions zsh > ~/.zfunc/_outline"
Enter
Type "fpath=(~/.zfunc $fpath) && autoload -Uz compinit && compinit -u"
Enter
Sleep 1s
Type "clear"
Enter
Sleep 500ms
Show

# 1. Resource-level completion: show all resources
Type "outline "
Sleep 500ms
Tab
Sleep 3s

# 2. Continue typing to drill into documents actions
Type "documents "
Sleep 500ms
Tab
Sleep 3s

# 3. Partial completion: "c" completes to "create"
Type "c"
Sleep 500ms
Tab
Sleep 3s
