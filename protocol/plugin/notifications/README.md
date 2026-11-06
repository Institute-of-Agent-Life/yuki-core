# Plugin notification publication

CORE-058 follows the fixed v4 notification manifest and original target ownership. The authenticated installation supplies the publisher namespace. A publication retains its original target entry and receiving stage. Plugin publication keeps human actor fields empty.

## Normal and failure sequences

- Two installed publishers submit the same external key. Each namespace receives its own immutable publication. Within one namespace, an exact replay returns the original publication, Main admission and delivery operations.
- Normalize the occurrence timestamp to UTC and canonicalize payload keys. Compare external source, event type, target, topic, summary, text, agent intent, wait-resume intent and ordered media identities. A conflicting replay fails before producing any new effect.
- Validate the current target grant and original sources inside the writer. Commit the publication and delivery intent together. A process kill before the plugin receives its reply leaves one durable original. Recovery reconciles that original key.
- Direct delivery uses the common Social operation and per-part receipts. Unknown delivery retains the original physical attempt and confirmed prefix. Replaying a publication cannot resend an unknown part.
- `ask_agent` admits a plugin-origin Main turn. A quiet result leaves a successful publication with zero public sends. The target Person or recent administrator never becomes its actor.
- `resume_waiting_work` can satisfy only an existing wait for this publisher and event type in the original authorized entry and conversation generation. An unrelated notification leaves the original Work waiting.
- Recheck target grants and receiving stages at dispatch and read boundaries. Revocation blocks further delivery and unauthorized receipt reads. Original accepted media identities remain replayable after their leases expire. A new publication cannot acquire an expired or erased lease.
- Reverse two accepted media handles, change topic, revoke a source while a response is pending and restart between publication commit and plugin acknowledgement. Compare immutable identities, physical receiver counts, Work identities and retained usage.

The core case uses independently installed SDK peers, controlled QQ and gateway receivers, and a loopback model. EXT-027 supplies the real subscription consumer. EXT-047 supplies other real notification consumers. Cross-repository consumption remains pending until those exact candidates are exercised.

## Subscription target administration

`notification.grant_target` and `notification.revoke_target` require a Host-issued original call handle, a stable mutation key and the actual human administrator in that execution lineage. A plugin-supplied actor or superuser field is rejected. The original receiving bot determines the grant. An omitted conversation defaults to the invocation entry when its Person or Space matches. The returned `conversation_id` is the field consumed by the fixed v4 subscription monitor.

Dynamic revocation overrides an installed target and survives restart. Replaying an earlier grant returns that historic receipt without undoing a later revoke. Other targets remain authorized across unrelated installation changes. New mutations require a live call in the current generation. Known receipts still require the original administrator and readable original sources. `notification.status` returns namespace-scoped actual `outbox_<state>` and `turn_<state>` counts.
