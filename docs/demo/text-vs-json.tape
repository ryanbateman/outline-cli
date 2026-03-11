# text-vs-json.tape — Show the difference between text and JSON output modes
#
# Demonstrates: default text table output vs --output json

Output docs/demo/text-vs-json.gif

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

# Setup: credentials and PATH (hidden)
Hide
Type "export OUTLINE_API_TOKEN=$OUTLINE_API_TOKEN"
Enter
Type "export OUTLINE_API_URL=$OUTLINE_API_URL"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 500ms
Show

# 1. Text output (default) — formatted table
Type "# Default output: human-readable table"
Enter
Sleep 500ms
Type "outline collections list"
Sleep 300ms
Enter
Sleep 3s

# 2. Text output with color forced on
Type "outline documents list --json "
Type@0 "'"
Type@0 '{"collectionId":"48c4642d-2241-4b09-9a8a-9521170b6276"}'
Type@0 "'"
Type " --color always"
Sleep 300ms
Enter
Sleep 3s

# 3. JSON output — machine-readable
Type "# JSON output: structured for agents and scripts"
Enter
Sleep 500ms
Type "outline collections list --output json --fields id,name"
Sleep 300ms
Enter
Sleep 4s
