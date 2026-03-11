# CONTEXT.md — Outline CLI Agent Workflow Guide

This file teaches AI agents how to use the Outline CLI effectively.
For security model and invocation conventions, see `AGENTS.md`.

## Quick Start

```bash
# List collections
outline collections list --output json --fields "id,name"

# Search documents
outline documents search --json '{"query": "onboarding"}' \
  --fields "document.id,document.title,context" --output json

# Get document details
outline documents info --json '{"id": "UUID"}' \
  --fields "id,title,text,url" --output json

# Create a document (always dry-run first)
outline documents create \
  --json '{"title": "New Doc", "collectionId": "UUID", "text": "# Content", "publish": true}' \
  --dry-run --output json

# Then execute
outline documents create \
  --json '{"title": "New Doc", "collectionId": "UUID", "text": "# Content", "publish": true}' \
  --output json
```

## Core Rules for Agents

### 1. Always use `--output json`

Without it, output is human-formatted text that will break your parsing.

### 2. Always use `--fields` on list/search calls

API responses include large blobs. Use field masks to limit payload size:

```bash
# Good: small response
outline documents list --json '{"collectionId": "..."}' \
  --fields "id,title,url,updatedAt" --output json

# Bad: huge response with full document text
outline documents list --json '{"collectionId": "..."}' --output json
```

### 3. Always `--dry-run` before create/update/delete

Validates the payload against the OpenAPI schema without hitting the API:

```bash
outline documents create --json '{"title": "Test"}' --dry-run --output json
# {"ok":true,"action":"create","validated":true,...}
```

### 4. Always confirm with user before delete

Never autonomously execute delete commands. Show the dry-run result and ask.

### 5. Use `--sanitize` when processing user-generated content

Strips Unicode control characters and prompt injection vectors from responses:

```bash
outline documents info --json '{"id": "..."}' --sanitize --output json
```

## Resource Workflows

### Document Lifecycle

1. **Discover collections first:**
   ```bash
   outline collections list --fields "id,name" --output json
   ```

2. **List documents in a collection:**
   ```bash
   outline documents list --json '{"collectionId": "UUID"}' \
     --fields "id,title,updatedAt" --output json
   ```

3. **Create a document:**
   ```bash
   # Validate
   outline documents create --json '{
     "title": "Q1 Planning",
     "collectionId": "UUID",
     "text": "# Agenda\n\n- Item 1\n- Item 2",
     "publish": true
   }' --dry-run --output json

   # Execute
   outline documents create --json '{...}' --output json --fields "id,title,url"
   ```

4. **Update a document:**
   ```bash
   outline documents update --json '{
     "id": "DOC-UUID",
     "title": "Updated Title",
     "text": "# New Content"
   }' --output json --fields "id,title,url"
   ```

5. **Delete a document (requires user confirmation):**
   ```bash
   outline documents delete --json '{"id": "DOC-UUID"}' --dry-run --output json
   # Show result to user, get confirmation, then:
   outline documents delete --json '{"id": "DOC-UUID"}' --output json
   ```

### Search

```bash
# Full-text search
outline documents search --json '{"query": "hiring process"}' \
  --fields "document.id,document.title,context" --output json

# Search within a collection
outline documents search --json '{
  "query": "budget",
  "collectionId": "UUID"
}' --fields "document.id,document.title,context" --output json

# Title-only search (faster)
outline documents search_titles --json '{"query": "onboarding"}' \
  --fields "id,title" --output json
```

Note: Search results have nested structure. Use dot-notation fields:
`document.id`, `document.title`, `context` (not `id`, `title`).

### Pagination

For complete result sets, use `--page-all` for NDJSON streaming:

```bash
outline documents list --json '{"collectionId": "UUID"}' \
  --fields "id,title" --page-all --output json
```

This streams one JSON object per line (NDJSON), handling pagination automatically.

## Schema Introspection

Discover method signatures without external docs:

```bash
outline schema documents.create --output json
outline schema collections.list --output json
```

Returns the full request body schema with all fields, types, and descriptions.

## MCP Mode

For agent frameworks that support MCP (Claude Code, Cursor, etc.):

```bash
outline mcp --expose documents,collections
```

This starts a JSON-RPC server over stdio. All tools are derived from the same
OpenAPI spec. Use `tools/list` to discover available methods and `tools/call`
to invoke them.

## Error Handling

All errors are JSON when `--output json` is set:

```json
{"ok": false, "error": "validation_error", "message": "id: Invalid", "code": 400}
```

Parse `ok` first, then `code`:
- `400`: Fix the request (bad input)
- `401`/`403`: Check credentials
- `404`: Resource not found
- `429`: Rate limited (CLI retries automatically with backoff)
- `5xx`: Server error (CLI retries automatically)

## Resource ID Format

Outline uses UUIDs: `550e8400-e29b-41d4-a716-446655440000`
Some document methods also accept URL slugs (e.g., `hDYep1TPAM`).
