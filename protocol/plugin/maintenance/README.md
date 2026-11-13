# Scoped maintenance candidate

An operator-installed maintenance policy binds the plugin identity to named methods, purposes, original Person scopes, concrete entries and permitted auxiliary routes. A callback cannot add an actor, private entry, evidence permission or model route. A subscription identity with no installed maintenance policy is denied before reading or reserving a request.

History pages return bounded original IDs and versions. Reading source text acquires an erasure obligation. Fact visibility and evidence readability are checked separately. A fact reader without an explicit evidence grant receives no quote or private conversation reference.

An extraction preview records the exact source versions and paid auxiliary receipt from which the proposed actions were derived. Applying an approved subset rechecks the original policy, subjects, entry scope, fact revisions and source erasure state in one writer transaction. Model I/O runs outside the writer. The same apply key returns its original receipt, including an empty action subset. A changed subset conflicts. Reembedding addresses an existing authorized fact and its current semantic version.

Acceptance exercises a subscription with an omitted actor, a visible fact with inaccessible evidence, a source erased after preview, a changed fact revision, an empty subset, a duplicate apply after restart, and a late model response after erasure. The named evidence is `maintenance-grants.json`. Scheduling, extraction algorithms and review UI belong to the extension consumers.

## Public request flow

The candidate exposes twelve `maintenance.*` methods through both SDK packages. The complete input and result shapes are in `methods.json`. Rust SDK `1.0.0-alpha.8` and Python SDK `1.0.0a8` carry these shapes with Host schema 2 and handshake schema 1.

1. An installed policy names a scope, its original Person and exact entry, allowed methods and purposes, auxiliary route names, embedding routes and an optional exact write subject. The plugin also implements `forget.owner`. Every call repeats its installed `scope` and `purpose`.
2. `history.page` returns original event/version pairs. `fact.page` returns fact/revision/source_version. `tool.page` requires evidence permission and returns actual confirmed operation receipts with their original Work ancestry. Select primary inputs and contextual inputs explicitly.
3. For fixed extraction, call `extract.preview` with `format=extraction`, an installed route, `mode` and original event references. The core builds the existing CORE-027 extraction request. For extension-owned review algorithms, call `model.open`, pass its `origin` to `llm.complete`, and retain the returned original request ID. Call `extract.preview` with that ID, `format=edits` and exactly the same event/fact/tool selection.
4. `extract.apply` accepts the original preview ID, a stable operation key and approved ordinals. Each ordinal uses its original idempotency key. After restart, additional approved ordinals can be applied from that paid preview. The core recognizes revisions written by that same preview while rejecting external source or evidence changes.
5. `audit` reads source and profile coverage without a model. `audit.apply` accepts its fingerprint and selected fact IDs to rebuild their FTS rows. `reembed` requires a separately installed semantic profile, its identity and the exact fact revision. Pending and unknown embeddings retain their original barrier and accounting.

The development SDK can request `integrity: true` on `maintenance.audit`.
Each visible fact then includes its lexical index state, missing evidence,
the number of event proofs checked and invalid within approved entries, and
vector coverage and last observed attempt state for each installed embedding
route. Findings omit fact and evidence text. A vector with a stale hash or
invalid shape is a diagnostic. For a lexical repair, pass the same flag and audit page
parameters to `maintenance.audit.apply`. The fingerprint binds the scoped
source and lexical row. An apply against a changed row requires a fresh scan.

An operator worker that saves reports requests `retain: true` on audit, apply
and reembed. Each audit item carries opaque `erasure_refs` for its original
dependencies. Apply returns the union for the approved facts. Reembed returns
the dependencies for the selected revision. A plain audit performs no retention
writes. The retained audit uses the same scan fingerprint and makes no model
request. The Host refuses retention if it cannot return the complete reference
set within the response limits.

Persist the references atomically with any retained local result. The original
`forget.request` watermark carries the matching `erasure_refs`. Remove the
local reports that reference them before acknowledging that watermark. The Host
continues to pass event IDs for other grants that already returned those IDs.
Large reference lists arrive in bounded batches under the same watermark.
Each batch requires the same successful cleanup proof before the Host records
the acknowledgment.
After erasure a retained apply key remains fenced and the original effect is
unavailable for replay. A lost callback leaves the obligation pending.

Fact summaries carry opaque event IDs needed to honor erasure obligations. Quotes and tool receipts require explicit evidence permission. Tool evidence remains attributed to the original human execution ancestry. Unattributed SELF executions do not acquire a substitute human author.

Pages contain up to eight items. Long fact bodies, evidence and paid previews expose a `content` descriptor. `maintenance.content` returns at most 12,288 bytes as base64, with offsets and the same SHA-256 on every page. `maintenance.dependencies` pages the complete original event set when its first 128 IDs do not fit the summary. Source changes invalidate both content versions and review eligibility.

The core validates original tool receipt versions alongside event and fact versions before model dispatch, publication and reviewed writes. Reading or selecting those inputs acquires SourceHold obligations. Erasure clears preview content and fences late writes while preserving operation IDs and known usage.

## Verification scope

The core case drives installed SDK subprocesses, real Rust Work children and loopback HTTP model/embedding peers. It covers closed historical entries, private SELF evidence, large content, scope denial, same-count source changes, partial apply/restart, selected FTS rebuild, semantic profile changes and late responses after erasure. True EXT-033, EXT-034, EXT-035, EXT-036 and EXT-048 consumption is pending.
