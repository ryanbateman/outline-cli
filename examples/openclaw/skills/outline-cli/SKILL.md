---
name: outline-cli
description: Manage an Outline knowledge base — search, create, update, and organize documents and collections
metadata: {"openclaw": {"requires": {"bins": ["outline"], "env": ["OUTLINE_API_TOKEN", "OUTLINE_API_URL"]}}}
---

# Outline CLI

The `outline` CLI provides full access to the Outline knowledge base API. All 17 resources
and 107 methods are generated dynamically from the OpenAPI spec at runtime. Every command
accepts `--json` with the raw API payload and returns structured JSON.

## Rules

1. **Always** use `--output json` for parseable output
2. **Always** use `--fields` on list/search calls to limit response size
3. **Always** `--dry-run` before create, update, or delete
4. **Never** delete without explicit user confirmation
5. Use `--sanitize` when processing user-generated content

## Discovery

List all resources and methods:

```bash
outline --help
```

Inspect a method's request schema before calling it:

```bash
outline schema documents.create
outline schema collections.list
```

## Common Operations

### List collections

```bash
outline collections list --output json --fields "id,name"
```

### List documents in a collection

```bash
outline documents list \
  --json '{"collectionId": "COLLECTION_UUID"}' \
  --fields "id,title,updatedAt" --output json
```

### Search documents

```bash
outline documents search \
  --json '{"query": "search terms"}' \
  --fields "document.id,document.title,context" --output json
```

Search within a specific collection:

```bash
outline documents search \
  --json '{"query": "budget", "collectionId": "COLLECTION_UUID"}' \
  --fields "document.id,document.title,context" --output json
```

Note: search results have nested structure. Use dot-notation fields:
`document.id`, `document.title`, `context` (not `id`, `title`).

### Read a document

```bash
outline documents info \
  --json '{"id": "DOCUMENT_UUID"}' \
  --fields "id,title,text,url" --output json
```

### Create a document

```bash
# Step 1: validate
outline documents create \
  --json '{"title": "Q1 Planning", "collectionId": "UUID", "text": "# Content", "publish": true}' \
  --dry-run --output json

# Step 2: execute
outline documents create \
  --json '{"title": "Q1 Planning", "collectionId": "UUID", "text": "# Content", "publish": true}' \
  --output json --fields "id,title,url"
```

### Update a document

```bash
# Step 1: validate
outline documents update \
  --json '{"id": "DOCUMENT_UUID", "text": "# Updated content"}' \
  --dry-run --output json

# Step 2: execute
outline documents update \
  --json '{"id": "DOCUMENT_UUID", "text": "# Updated content"}' \
  --output json --fields "id,title,url"
```

### Delete a document (requires user confirmation)

```bash
# Step 1: dry-run and show result to user
outline documents delete --json '{"id": "DOCUMENT_UUID"}' --dry-run --output json

# Step 2: only after user confirms
outline documents delete --json '{"id": "DOCUMENT_UUID"}' --output json
```

### Comments

```bash
# List comments on a document
outline comments list \
  --json '{"documentId": "DOCUMENT_UUID"}' --output json

# Create a comment
outline comments create \
  --json '{"documentId": "DOCUMENT_UUID", "data": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "Comment text"}]}]}}' \
  --output json
```

## Pagination

Use `--page-all` to fetch all results (streams one JSON object per line):

```bash
outline documents list \
  --json '{"collectionId": "UUID"}' \
  --fields "id,title" --page-all --output json
```

Default page size is 25. Set `limit` explicitly to control batch size:

```bash
outline documents list \
  --json '{"collectionId": "UUID", "limit": 100}' \
  --page-all --output json
```

## Error Handling

All errors are JSON when `--output json` is set:

```json
{"ok": false, "error": "validation_error", "message": "id: Invalid", "code": 400}
```

Parse `ok` first, then `code`:
- `400`: bad input — fix the request, do not retry
- `401`/`403`: auth failure — check credentials
- `404`: resource not found — verify the ID
- `429`: rate limited — CLI retries automatically with backoff
- `5xx`: server error — CLI retries automatically with backoff

## Resource ID Format

Outline uses UUIDs: `550e8400-e29b-41d4-a716-446655440000`.
Some document methods also accept URL slugs.
