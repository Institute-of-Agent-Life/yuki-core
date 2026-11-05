# Code composition candidate

The installed runtime pins its tool, engine, launcher, codec and ABI. An accepted Work and its original frozen manifest authorize each child. `main.code_tool` selects an installed runtime. The core stores opaque snapshots and delegates interpretation to that runtime.

At T0, the runtime uploads bounded chunks through `code.snapshot.put`. The core checks the complete hash outside its writer. T1 atomically publishes the parent checkpoint, snapshot and original child intent. The business key is the original parent operation plus child ordinal. The stored VM position includes feed index and engine call ID. Repeating arguments at another ordinal creates another operation.

T2 records entry into the common dispatcher. Existing operation and physical attempt records determine whether a boundary was crossed. T3 saves the original typed result. `tool.resume` retrieves it. A feed acknowledgement publishes the post-feed snapshot with the expected checkpoint. Cold recovery sends `code.resume` with the saved snapshot and child records. Missing or incompatible snapshots yield an unavailable result. A physically dispatched unknown child remains unknown. A parent already paired or closed cannot accept more children.

`tool.catalog` and `tool.schema` use the original manifest. `code.sources` pages the actual source IDs of the bound parent, allowing the runtime to associate retained private items with later erasure obligations. Grants and current source authority are checked for every callback. A real ancestor cycle is rejected. Normal deep chains retain their own operations without a fixed depth threshold. Per-parent byte, pending-future and suspension limits are explicit resource limits.

The parent composition and Work lifecycle calls consume zero business tool slots. `read_tool_artifact` records an actual read and receipt with zero business charge. Other child calls and direct calls share one root counter and the same dispatcher. An original operation reserves once across retries and process recovery.

Acceptance cuts terminate the real core after T1, after a child physical dispatch, after T3 and after feed acknowledgement. They compare original IDs, VM positions, root counts, receipts and receiver counts. Local SDK fixtures exercise opaque snapshot transport. The matching EXT-023 runtime and EXT-024 real VM witness are required before protocol freeze.

`main.root_tool_calls` optionally limits admitted business operations across the original root. The default records counts with an unbounded limit. `/client/v1/works` and the operator usage result report this same counter. A continued segment, child or recovered VM keeps the root limit. Legacy settled model tool intents are carried into the counter, with an unknown admission time.

Snapshots default to 8 MiB, at most 16 pending futures and 1024 suspensions per parent. The configured snapshot ceiling can range from 64 KiB to 128 MiB. Two incomplete uploads may be staged at once. The host validates hashes outside the writer before publishing references, and reclaims superseded bodies after feed acknowledgement. Recovery sends bounded pending-child metadata. Each original result is retrieved by ordinal, so a long completed prefix does not enlarge a single RPC frame. Code calls default to a 30-second response watchdog, configurable through the pinned runtime up to 600 seconds.
