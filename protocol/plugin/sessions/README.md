# Isolated plugin sessions

CORE-051 continues the fixed v4 `AgentSessionFacade` under an authenticated Host namespace. Its original scope is a real caller, a real group or the installed plugin itself. A plugin-only session keeps its Conversation and Person references null. The provider request contains visible user/assistant history and the approved context metadata. Main tools, Main history, Memory and hidden reasoning stay outside this transcript.

## Normal and failure sequences

- Create under `(plugin, session ID)`, using a stable create key. Two installed plugins can use the same UUID independently. Resolve a real call/event authority through the existing Host authorization path. Actorless creation requires an explicit installed background-session grant.
- Append a user message and admit its original auxiliary logical request in one writer transaction. Physical I/O follows commit through CORE-050. The original response appends once at its reserved session sequence. A retry returns the same turn and receipt.
- Resume a known response after a process kill. Retain an unknown attempt without buying a replacement. Concurrent callers use the expected session epoch and one pending turn, with no lost sequence or interleaved history.
- Page only the original session's visible transcript. Long content uses digest-bound byte pages. A model ledger receipt is accepted only when the session's original turn owns it.
- Reset increments the epoch, clears old content and retains original operation keys. Close ends new turns. Reset, close, grant revocation and source erasure fence in-flight publication. Usage and original IDs survive content removal.
- Forget a real source while an append or response is pending. Original text, frozen model inputs and late output are redacted. An unrelated plugin/session remains readable.
- Durable sessions survive a core restart. Ephemeral sessions expire at the next core boot. Original initiating Work budgets remain the parent budget when a real Host call initiated the request.

Validation uses two independently installed SDK peers, real loopback provider requests and kill/restart cuts. CORE-054 owns old-session import. EXT-047 supplies the real external consumer evidence.

## Candidate contract

The eight methods are `session.create`, `session.page`, `session.content`, `session.sources`, `session.append`, `session.run`, `session.reset` and `session.close`. The IDL binds every method to its explicit grant. Installed `sessions` policy selects allowed auxiliary routes, context profiles and actorless background authority. A route that only accepts session origins can have no chat entry.

A supplied `authority` contains an actual Host call handle or observed event. User scope retains the original Person. A group session created by a real Person retains that Person as well as the original group. Omitted authority is accepted only by an installed background-session policy. A session ID is canonicalized UUID text under the authenticated plugin namespace.

The fixed v4 ownership predicates permit the same Person to continue a user session from an authorized group entry. A group session permits another topic in the same group when the original Person matches. A plugin-owned background session remains shared within its authenticated namespace. A real caller contributes its own sender and source references without becoming that session's owner. An actorless session created from a specific SELF entry keeps that entry constraint.

Each mutation carries an original key. Within a session, that key identifies one append, run, reset or close operation. Append, run, reset and close also carry the expected epoch. A known paid run can be reconciled after its parent finishes. A new run still needs fresh invocation authority. Sessions retain the existing v4 model window of the latest 200 visible user/assistant messages, while the public page API exposes the stored transcript. Imported tool records have a distinct readable role and stay outside the session's model input. CORE-054 owns their import.

Overlapping calls with the same run key share one user sequence, logical request and physical attempt. A caller can receive `prepared` while the original callback is in flight. It reconciles that same key for the final receipt. Recovery tests overlap the restarted inbox replay with an explicit caller retry and wait for the original durable settlement.

Content-bearing responses include the exact event dependency map. The first 128 IDs fit the response. `session.sources` pages larger sets under the same version, allowing a consumer to bind its cached content to `forget.request` before acknowledging erasure. Text pages use SHA-256 and base64 byte offsets. Long UTF-8 characters remain intact when the consumer rejoins the bytes.

`session.run` owns its original auxiliary receipt. Public `llm.result` cannot turn a session's model ledger into an alternate history reader. Only visible reply text enters the transcript, and provider reasoning stays outside subsequent session requests. Reset and erasure retain original IDs and known usage while removing old message and model content.

The case exercises Chat, OpenAI Responses, both DeepSeek Responses dialects, Anthropic Messages and Gemini through physical loopback requests. A known nonterminal Messages response records a failed session turn and preserves its physical attempt. Replaying that key reconciles the original result. Instructions and user input follow v4 whitespace normalization, including Python's four C0 separator characters.

Candidate schema 1 uses Rust SDK `1.0.0-alpha.9` and Python SDK `1.0.0a9`. True EXT-047 consumption remains pending. The core fixtures exercise namespace isolation, original Work budgets, migration of a populated schema-33 snapshot, physical provider boundaries, source erasure and three process-kill cuts.
