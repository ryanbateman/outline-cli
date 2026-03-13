# safety.tape — Safety rails: dry-run, validation, sanitization
#
# Demonstrates: --dry-run before mutations, input validation catching
# bad UUIDs, and schema validation catching missing required fields

Output docs/demo/safety.gif

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

# 1. Dry-run: validate without executing
Type "outline documents create --json "
Type@0 "'"
Type@0 '{"title":"Test","collectionId":"48c4642d-2241-4b09-9a8a-9521170b6276","text":"# Hello"}'
Type@0 "'"
Type " --dry-run"
Sleep 300ms
Enter
Sleep 3s

# 2. Input validation: bad UUID
Type "outline documents info --json "
Type@0 "'"
Type@0 '{"id":"not-a-valid-uuid?inject=true"}'
Type@0 "'"
Sleep 300ms
Enter
Sleep 3s

# 3. Schema validation: missing required field
Type "outline documents create --json "
Type@0 "'"
Type@0 '{"title":"No Collection"}'
Type@0 "'"
Type " --dry-run"
Sleep 300ms
Enter
Sleep 4s
