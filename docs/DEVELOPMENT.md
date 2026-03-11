# Development

## Architecture

`outline-cli` is a Rust binary that builds its entire command tree at runtime from Outline's [OpenAPI spec](https://github.com/outline/openapi). There is no code generation step — the spec is the source of truth for available resources, methods, request schemas, and response types.

**Spec loading** follows a three-tier strategy:

1. **Fetch** `spec3.json` from GitHub (async, on first run or when cache expires)
2. **Cache** locally with a 24-hour TTL (`~/.cache/outline/spec3.json`)
3. **Embedded fallback** compiled into the binary via `include_str!()` for offline use

From the spec, the CLI derives:

- **17 resources** (documents, collections, users, groups, etc.)
- **107 methods** (all POST — Outline's API is RPC-style, not REST)
- Request body schemas for `--json` validation
- Response schemas for `--fields` dot-notation filtering

**Key design decisions:**

- `clap 4.5+` requires `&'static str` for command names — solved by leaking strings (`Box::leak`) since the command tree lives for process lifetime
- All validation is **field-aware**: ID fields (`format: uuid`) get strict checks (reject `?`, `#`, `%`, double-encoding); content fields get only control-character rejection. This follows the [gws](https://github.com/googleworkspace/cli) reference implementation.
- MCP server mode reuses the same spec parser and executor, generating `tools/list` responses dynamically from the OpenAPI spec

## Building

```bash
cargo build            # debug
cargo build --release  # optimized
```

The binary is named `outline` (not `outline-cli`).

## Testing

```bash
cargo test
```

**219 tests** across four test targets:

| Target | Count | What |
|--------|-------|------|
| lib (unit) | 99 | Spec parsing, schema validation, `$ref` resolution, input hardening, output formatting |
| bin (unit) | 106 | Command building, credential loading, executor logic, helper dispatch |
| integration: retry | 6 | Wiremock-based HTTP retry behavior (429, 5xx, Retry-After) |
| integration: MCP | 8 | Process-spawning MCP conformance (initialize, tools/list, tools/call over stdio JSON-RPC) |

Zero compiler warnings.

## Project Structure

```
src/
  main.rs              # Entrypoint: spec load -> command tree -> dispatch
  lib.rs               # Library crate for integration tests
  spec/
    loader.rs          # Fetch, cache, embed fallback
    parser.rs          # OpenAPI spec -> ApiSpec (resources, methods)
    resolver.rs        # $ref resolution with cycle detection
    validator.rs       # Schema validation (types, required, format, allOf, oneOf)
  commands/
    builder.rs         # Dynamic clap command tree from spec + helper injection
  executor/
    http.rs            # HTTP POST with retry, backoff, ApiResponse
  output/
    formatter.rs       # JSON/text output, dot-notation --fields filtering
  auth/
    credentials.rs     # Env var + config file credential loading
  validate/
    input.rs           # Field-aware input validation (ID vs content)
  mcp/
    server.rs          # MCP server (rmcp SDK, ServerHandler impl)
  helpers/
    mod.rs             # Helper trait + registry
    documents.rs       # +new, +edit, +search implementations
api/
  spec3.json           # Embedded OpenAPI spec
tests/
  retry_test.rs        # Wiremock integration tests
  mcp_test.rs          # MCP conformance tests
docs/
  DEVELOPMENT.md       # This file
plan.md                # Canonical design document (all phases, rationale)
AGENTS.md              # Agent security model and invocation conventions
CONTEXT.md             # Agent workflow guide (how to use each resource)
```

## Design Document

The full design rationale, phase breakdown, and architecture decisions are in [`plan.md`](../plan.md). Read that first when continuing work on this project.
