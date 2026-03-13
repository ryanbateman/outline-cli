# outline

This is an unofficial CLI tool for [Outline](https://getoutline.com), the open source knowledge base based primarily on Markdown files. It is built on two principles: make it easy to use your favourite terminal-based editor to work with and publish Markdown documents in Outline, and make it easy for agents to work with these same documents as safely and securely as possible.  

I built it around these principles because I found I was exchanging a lot of Markdown files with agents, and the logical question after the first two ("Why did I choose a life like this?" and "How can I make it stop?") was "How can I streamline this process?".  

This README contains the following sections:  
- [How/why people can use this tool](#howwhy-people-can-use-this-tool)
- [How/why agents should use this tool](#howwhy-agents-should-use-this-tool)
- [Raw API usage](#raw-api-usage)
- [Integrations](#integrations) (OpenCode, OpenClaw)
- [Install](#install-the-tool) and [configure](#configure-the-tool)

## How/why people can use this tool

There were a couple of things I wanted to do when aiming to make a useful CLI tool for working with Markdown in Outline.  
The first was to make it user-friendly and easy to navigate - at its best, working in a terminal feels like knowing all the keyboard shortcuts. The downside of that, of course, is having to learn them all.  
To try to make this a bit easier, this tool ships with both an interactive helper and autocompletion support. These are the primary ways a user is expected to work with it.  
The tool is also focused on keeping you in your terminal (and your favourite editor), even as you create, search for, and publish documents to Outline. 

### Helper commands

The fastest way to work with documents interactively is with the helper. There are three primary helper commands (for the moment).

```bash
outline documents +new      # Create a document (prompts for collection, title, editor)
outline documents +search   # Interactive search with result selection
outline documents +edit     # Edit an existing document in $EDITOR
```

![Creating a document with +new](docs/demo/helper-new.gif)

**`outline documents +new`** walks you through creating a document:
1. Select a collection from a list
2. Enter a title
3. Opens your `$EDITOR` to write the content
4. Publishes on save

To quickly find and edit a document, you can use the +search helper. 

![Interactive search with +search](docs/demo/helper-search.gif)

**`outline documents +search`** gives you interactive search:
1. Enter a search query
2. Browse matching results with context snippets
3. Select a document, then choose an action:
   - **Edit in $EDITOR** — fetch, edit, and save changes back
   - **Open in browser** — open the document in Outline
   - **Show details** — print ID, title, and URL

To jump straight into a specific document, you can use the ID. (Admittedly this bit could be a little more user-friendly.)

**`outline documents +edit --id <uuid>`**:
1. Fetches the document content
2. Opens it in your `$EDITOR`
3. Saves changes back to Outline on exit


### Shell completions

The tool ships with the ability to generate autocompletions for your favourite shell. Definitely recommend if you want the fastest, most streamlined way to jump in and out of documents.


To set them up, run the tool with the 'completions' command and your desired shell.  

```bash
# zsh (add fpath line to ~/.zshrc before compinit)
mkdir -p ~/.zfunc
outline completions zsh > ~/.zfunc/_outline
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit

# bash
outline completions bash > /etc/bash_completion.d/outline

# fish
outline completions fish > ~/.config/fish/completions/outline.fish
```

![Shell completions](docs/demo/completions.gif)

## How/why agents should use this tool

The CLI tool was built for agents following the principles from [You Need to Rewrite Your CLI for AI Agents](https://justin.poehnelt.com/posts/rewrite-your-cli-for-ai-agents/) and modeled after the [Google Workspace CLI](https://github.com/googleworkspace/cli) reference implementation.
The entire command tree is generated at runtime from Outline's [OpenAPI spec](https://github.com/outline/openapi) — no code generation, no stale wrappers. This may seem like overkill for Outline (as opposed to necessary for something like `gws`). It probably is. Largely I wanted to validate some of the principles, to reduce the overhead of unnecessary token use through large markdown content being exchanged when it isn't always needed, and to build something that 'spoke' agent.  
As such, every command accepts `--json` with the raw API payload, returns structured JSON, and validates inputs against the OpenAPI schema, amongst other things.

### Conventions for agents

```bash
# Always use --output json for parseable output
outline documents list --json '...' --output json

# Always use --fields to limit response size (agents pay per token)
outline documents list --json '...' --fields "id,title,url" --output json

# Always --dry-run before create/update/delete
outline documents delete --json '{"id": "..."}' --dry-run --output json

# --page-all streams NDJSON (one JSON object per line) for large result sets
outline documents list --json '...' --page-all --output json
```

Safety features built for agents:

- **`--dry-run`** validates `--json` payloads against the OpenAPI schema without hitting the API
- **`--sanitize`** strips Unicode control characters and prompt injection vectors from responses
- **Retry with backoff** for 429 (rate limit) and 5xx errors, respects `Retry-After`
- **Field-aware input validation** rejects malformed IDs (hallucinated UUIDs with `?`, `#`, `%`) while allowing legitimate content in text fields

![Safety rails](docs/demo/safety.gif)

Full agent conventions are documented in [AGENTS.md](AGENTS.md). Workflow examples are in [CONTEXT.md](CONTEXT.md).

## Raw API Usage

For the moment, doing things like listing documents in collections and seeing attachments requires a human to input `json` rather than use a helper. Not user-friendly. This will likely change and improve over time, but here are the basics of that kind of usage.  

![Text vs JSON output](docs/demo/text-vs-json.gif)

### Discover what's available

Every command and method comes directly from the API spec. Start by listing resources:

```bash
outline --help
```

Inspect a specific method's schema before calling it:

```bash
outline schema documents.create
outline schema collections.list
```

![Schema introspection](docs/demo/schema.gif)

### List and search

```bash
# List collections
outline collections list --output json --fields "id,name"

# List documents in a collection
outline documents list \
  --json '{"collectionId": "..."}' \
  --fields "id,title,updatedAt" \
  --output json

# Search
outline documents search \
  --json '{"query": "onboarding"}' \
  --fields "document.id,document.title,context" \
  --output json
```

Use `--page-all` to fetch all results instead of just the first page:

```bash
outline documents list \
  --json '{"collectionId": "..."}' \
  --fields "id,title" \
  --page-all \
  --output json
```

### Create and update

```bash
outline documents create \
  --json '{"title": "Q1 Planning", "collectionId": "...", "text": "# Agenda", "publish": true}' \
  --output json
```

### MCP Server

Largely for completeness' sake, the CLI can also expose itself as an [MCP](https://modelcontextprotocol.io) server over stdio. Outline itself already has [MCP support](https://docs.getoutline.com/s/guide/doc/mcp-6j9jtENNKL), but this gives agents access through the CLI's validation and safety layer. Similar to the CLI itself, the MCP tools are generated dynamically from the OpenAPI spec — `tools/list` returns every available method with its `inputSchema`.
For some additional security, you can use `--expose` to limit which resources are available to the agent. Omit it to expose all 17 resources.

### Integrations

#### OpenCode integration

[OpenCode](https://opencode.ai) supports two complementary integration paths: MCP for direct tool access, and Agent Skills for teaching the agent how to use the CLI.

##### MCP server

To use the CLI tool as an MCP server with OpenCode, add the following to your `opencode.json`:

```json
{
  "mcp": {
    "outline": {
      "type": "local",
      "command": ["outline", "mcp", "--expose", "documents,collections"],
      "environment": {
        "OUTLINE_API_TOKEN": "{env:OUTLINE_API_TOKEN}",
        "OUTLINE_API_URL": "{env:OUTLINE_API_URL}"
      }
    }
  }
}
```

The agent gets direct access to Outline methods as MCP tools — no shell escaping, no output parsing.

##### Agent Skill

To use the CLI tool as an Agent Skill in OpenCode, create `.opencode/skills/outline/SKILL.md` in your project (or `~/.config/opencode/skills/outline/SKILL.md` for global access):

```markdown
---
name: outline
description: Manage Outline knowledge base — search, create, and update documents via the outline CLI
---

## Available commands

The `outline` CLI provides access to the full Outline API. Key operations:

- `outline collections list --output json --fields "id,name"` — discover collections
- `outline documents search --json '{"query": "..."}' --fields "document.id,document.title,context" --output json` — search
- `outline documents info --json '{"id": "..."}' --fields "id,title,text" --output json` — read a document
- `outline documents create --json '{"title": "...", "collectionId": "...", "text": "...", "publish": true}' --output json` — create
- `outline schema <resource>.<method>` — inspect any method's request/response schema

## Rules

- Always use `--output json` and `--fields` to keep responses small
- Always `--dry-run` before create, update, or delete
- Never delete without user confirmation
- Use `--sanitize` when processing user-generated content
```

The skill teaches the agent *when* and *how* to invoke the CLI. It is critical to include this to reduce token usage and to ensure (as best as possible) that the agent uses the tool correctly.

#### OpenClaw integration

[OpenClaw](https://openclaw.ai) is a personal AI assistant that supports skills — `SKILL.md` files with YAML frontmatter that teach the agent how to use tools. Skills are loaded from `~/.openclaw/skills/<name>/SKILL.md` (shared across agents) or `<workspace>/skills/<name>/SKILL.md` (per-agent).

Install the skill:

```bash
mkdir -p ~/.openclaw/skills/outline-cli
cp examples/openclaw/skills/outline-cli/SKILL.md ~/.openclaw/skills/outline-cli/
```

Add credentials to `~/.openclaw/openclaw.json`:

```json
{
  "skills": {
    "entries": {
      "outline-cli": {
        "enabled": true,
        "env": {
          "OUTLINE_API_TOKEN": "ol_api_YOUR_TOKEN_HERE",
          "OUTLINE_API_URL": "https://your-instance.getoutline.com/api"
        }
      }
    }
  }
}
```

Start a new OpenClaw session to pick up the skill. The `outline` binary must be on `PATH`.

See [`examples/openclaw/`](examples/openclaw/) for ready-to-use skill and configuration files.


## Install the tool

From source (requires Rust 1.85+):

```bash
cargo install --path .
```

Verify:

```bash
outline --help
```

## Configure the tool

Set your Outline API credentials as environment variables:

```bash
export OUTLINE_API_TOKEN="ol_api_..."
export OUTLINE_API_URL="https://your-instance.getoutline.com/api"
```

For Outline cloud, `OUTLINE_API_URL` defaults to `https://app.getoutline.com/api`.

Alternatively, create a config file at `~/.config/outline/credentials.json`:

```json
{
  "api_token": "ol_api_...",
  "api_url": "https://your-instance.getoutline.com/api"
}
```

## Development

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for architecture, building, testing, and project structure.
This tool was built using OpenCode and Claude Opus 4.6. I do not know Rust but I know good development principles and made every effort to review during development and test for safety. I bear no responsibility for use of the tool and its consequences. 

## License

[MIT](LICENSE)
