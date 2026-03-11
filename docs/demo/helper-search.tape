# helper-search.tape — Interactive document search with action menu
#
# Demonstrates: +search helper with dialoguer prompts, result selection,
# and colorized detail output (Title, ID, URL, Match)

Output docs/demo/helper-search.gif

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

# Setup: credentials (hidden from recording)
Hide
Type "export OUTLINE_API_TOKEN=$OUTLINE_API_TOKEN"
Enter
Type "export OUTLINE_API_URL=$OUTLINE_API_URL"
Enter
Type "export OUTLINE_DEMO_URL=https://demo.getoutline.com"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 500ms
Show

# 1. Launch interactive search
Type "outline documents +search"
Sleep 300ms
Enter

# 2. Wait for search query prompt, type query
Wait+Screen /Search query/
Sleep 300ms
Type "outline CLI README"
Sleep 500ms
Enter

# 3. Wait for results list — first item is "outline-cli README", pre-selected
Wait+Screen /Select document/
Sleep 2s
Enter

# 4. Wait for action menu, select "Show details" (index 2)
Wait+Screen /Action for/
Sleep 1s
Down
Down
Sleep 500ms
Enter

# 5. Let viewer read the colorized details output
Sleep 5s
