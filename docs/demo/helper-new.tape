# helper-new.tape — Interactive document creation with nano editor
#
# Demonstrates: +new helper with collection picker, title prompt,
# nano editor for content, colorized summary, and creation result

Output docs/demo/helper-new.gif

Require outline
Require nano

Set Shell "zsh"
Set FontSize 16
Set Width 1000
Set Height 600
Set Theme "Catppuccin Mocha"
Set BorderRadius 8
Set Padding 20
Set Margin 20
Set MarginFill "#1e1e2e"
Set TypingSpeed 75ms
Set CursorBlink false

# Setup: credentials and editor (hidden from recording)
Hide
Type "export OUTLINE_API_TOKEN=$OUTLINE_API_TOKEN"
Enter
Type "export OUTLINE_API_URL=$OUTLINE_API_URL"
Enter
Type "export OUTLINE_DEMO_URL=https://demo.getoutline.com"
Enter
Type "export EDITOR=nano"
Enter
Sleep 500ms
Type "clear"
Enter
Sleep 500ms
Show

# 1. Launch interactive new document
Type "outline documents +new"
Sleep 300ms
Enter

# 2. Wait for collection picker, select "Welcome" (index 2)
Wait+Screen /Select collection/
Sleep 1s
Down
Down
Sleep 500ms
Enter

# 3. Wait for title prompt, type title
Wait+Screen /Document title/
Sleep 300ms
Type "Meeting Notes"
Sleep 300ms
Enter

# 4. Nano opens with "# Meeting Notes\n\n" pre-filled
#    Cursor starts at line 1 col 1 — navigate to end of content
Sleep 2s
Down
Down
Down

# 5. Type document content in nano
Type "## Attendees"
Enter
Enter
Type "- Product team"
Enter
Type "- Engineering"
Enter
Enter
Type "## Agenda"
Enter
Enter
Type "1. Q1 review"
Enter
Type "2. Q2 planning"
Sleep 1s

# 6. Save and exit nano: Ctrl+O, Enter (confirm), Ctrl+X (exit)
Ctrl+O
Sleep 500ms
Enter
Sleep 500ms
Ctrl+X

# 7. Wait for document summary to appear
Wait+Screen /Document Summary/
Sleep 3s

# 8. Confirm creation (default is yes, just press Enter)
Enter

# 9. Let viewer read the success output (checkmark, title, URL)
Sleep 5s
