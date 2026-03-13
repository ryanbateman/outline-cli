# schema.tape — Schema introspection for agent discovery
#
# Demonstrates: `outline schema` command that lets agents discover
# API method signatures at runtime without external docs

Output docs/demo/schema.gif

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

# 1. Schema in text mode (human-readable summary)
Type "outline schema documents.create"
Sleep 300ms
Enter
Sleep 4s

# 2. Schema in JSON mode (full OpenAPI schema for agents)
Type "outline schema documents.search --output json"
Sleep 300ms
Enter
Sleep 5s
