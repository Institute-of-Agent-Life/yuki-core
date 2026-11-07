# Optional Tavily plugin (archived development path)

The current EXT-029 acceptance route uses the selected model’s server-side web search. [Native search configuration](native-web-search.md).

A deployment selects Tavily explicitly. Native search and the external tool have separate names. A failed native search never requests an external permit. The extension owns the credentials, provider request and result translation. The Host owns the original operation, authority and root ledger.

Main offers this tool to a new execution after the plugin completes its handshake and the operator has approved all three grants. A plugin that becomes available later can serve the next execution. If a grant is revoked or the plugin disconnects after the model received its tool list, the Host refuses dispatch before reserving a provider call.

1. On the original `tool.call`, send `search.begin` with its `handle`, `provider: "tavily"` and `purpose: "web_search"`. The Host checks the approved installation, tool, current search mode and original actor. It commits one root request slot and a physical attempt in the same transaction. Token reservation is zero for this non-model request.
2. Only a receipt with `dispatch_now: true` permits one HTTP request to the returned endpoint. Disable automatic retries and redirects. A duplicate begin returns the original attempt with no dispatch authority.
3. Before replying to `tool.call`, send `search.finish` with the original handle and attempt. For a complete response send `outcome: "response"`, the HTTP `status`, `response_sha256` of the received bytes and optional `usage` (`credits` or `billed_microusd` when reported). Omit unknown amounts. A 429 records the response and grants no retry.
4. A lost or incomplete response takes `outcome: "unknown"`. A disconnect before finish also leaves the attempt unknown. After restart, `search.status({"handle": …})` returns the original attempt and known costs under the same deployment. It never grants a send. A different handle, deployment, or revoked scope cannot claim that receipt.

Minimal search settings for a new installation (replace the executable and select the real endpoint). The operator approves the listed grants. The tool schema below is a minimal consumer example.

```toml
[search]
mode = "tavily"

[[plugins]]
id = "tavily"
namespace = "tavily"
program = "/absolute/path/to/extension"
grants = ["tool.call", "forget.owner", "search.external"]

[plugins.external_search]
tool = "tavily_search"
provider = "tavily"
purpose = "web_search"
endpoint = "https://api.tavily.com/search"

[[plugins.tools]]
name = "tavily_search"
description = "Search using the configured Tavily provider"
cacheable = false
parallel_read = false
code_direct = false
schema = { type = "object", required = ["query"], properties = { query = { type = "string" } }, additionalProperties = false }
```

Loopback provider tests cover success, 429, lost response, shutdown before receipt, exhausted budget, denied handles and native failure without fallback. The signed development candidate carries the actual replay receipts. The six-method stable 1.0 base is separately qualified.
