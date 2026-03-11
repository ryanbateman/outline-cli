# AGENTS.md — Outline CLI

This file is for AI/LLM agents using the `outline` CLI. It encodes invariants and
conventions that aren't obvious from `--help` output.

## Security Model

This CLI is frequently invoked by AI/LLM agents. **Always assume inputs can be adversarial.**

Input validation is **field-aware** (following the gws reference implementation pattern):

- **ID fields** (`format: uuid` or named `id`/`*Id`): strict validation — reject `?`, `#`, `%`,
  double-encoded strings, and control characters
- **Content fields** (`text`, `title`, `description`, `query`, etc.): control-character rejection
  only — `%`, `?`, `#` are legitimate in document text and search queries
- Control characters below ASCII `0x20` are rejected in all fields (except `\n`, `\r`, `\t`)
- Field roles are derived from the OpenAPI schema at runtime

## Invocation Conventions

### Prefer `--json` over individual flags

Agents should use `--json` with the raw API payload rather than per-field flags:

```bash
# Preferred (agent-first)
outline documents create --json '{
  "title": "Q1 Planning",
  "collectionId": "550e8400-e29b-41d4-a716-446655440000",
  "text": "# Agenda\n...",
  "publish": true
}'

# Human convenience (avoid in automation)
outline documents create --title "Q1 Planning" --collection abc123 --file agenda.md
```

### Always use `--output json`

All structured output requires `--output json`. Without it, output may include ANSI colors
and human-formatted tables that break parsing.

```bash
outline documents list --json '{"collectionId": "..."}' --output json
```

### Always use `--fields` on list/search calls

API responses include large blobs by default. Use field masks to limit payload size:

```bash
outline documents list \
  --json '{"collectionId": "..."}' \
  --fields "id,title,url,updatedAt" \
  --output json
```

### Use `--page-all` for complete result sets

Without `--page-all`, results are paginated and truncated. Use `--page-all` for streaming
NDJSON output (one JSON object per line):

```bash
outline documents list \
  --json '{"collectionId": "..."}' \
  --fields "id,title,url" \
  --page-all \
  --output json
```

## Mutating Operations (create / update / delete)

### Run `--dry-run` before destructive operations

For create, update, and delete, always validate first with `--dry-run`:

```bash
# Step 1: Validate
outline documents delete --json '{"id": "abc123"}' --dry-run
# {"ok":true,"action":"delete","resource":"document","id":"abc123","validated":true}

# Step 2: Execute after confirming
outline documents delete --json '{"id": "abc123"}' --output json
```

**Never skip `--dry-run` for delete operations.**

### Confirm with the user before executing deletes

Do not autonomously execute delete commands without explicit user confirmation.

## Schema Introspection

Agents can discover method signatures at runtime without reading external docs:

```bash
outline schema documents.create
outline schema collections.list
```

Output is the OpenAPI method schema as JSON (request body, response types, required scopes).

## Auth

Credentials are read from environment variables (preferred for headless/agent use):

```bash
export OUTLINE_API_TOKEN="ol_api_<38 alphanumeric chars>"
export OUTLINE_API_URL="https://your-instance.getoutline.com"  # omit for cloud
```

Config file fallback: `~/.config/outline/credentials.json`

## Error Responses

All errors are returned as JSON when `--output json` is set:

```json
{"ok": false, "error": "Not Found", "code": 404}
```

Agents should parse `ok` first, then `code` for retry logic:
- `400`: Invalid input — do not retry, fix the request
- `401`/`403`: Auth failure — check token/scopes
- `404`: Resource not found — verify the ID
- `429`: Rate limited — retry after `Retry-After` header value
- `5xx`: Server error — retry with exponential backoff

## MCP Mode

The CLI can be exposed as an MCP (Model Context Protocol) server over stdio:

```bash
outline mcp --expose documents,collections
```

Use MCP mode to avoid shell escaping issues in agent frameworks that support JSON-RPC.

## Resource ID Format

Outline resource IDs are UUIDs (`550e8400-e29b-41d4-a716-446655440000` format).
API key format: `ol_api_` prefix + 38 alphanumeric characters.

## Pagination

Outline uses `limit`/`offset` params. Responses include `pagination.nextPath` when more
results exist. The `--page-all` flag handles this automatically, streaming NDJSON.

Default `limit` is 25 if not specified. Set explicitly to control batch size:

```bash
outline documents list --json '{"collectionId": "...", "limit": 100}' --page-all --output json
```
