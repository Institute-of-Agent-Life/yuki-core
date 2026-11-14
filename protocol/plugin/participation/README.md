# Participation candidate

An installed policy names the group entries a plugin may inspect or promote. SELF proposals additionally pin an explicit receiving SELF stage for each permitted scene. Every callback rechecks the current installation, source, entry and route.

Human promotion addresses an existing internal event ID. It revalidates the original envelope and identity in the writer, then uses the common admission path. Rule and model-review submissions for the same event retain one accepted source. The ordinary Main scheduler routes that source to the original matching Work or wait.

A SELF proposal has a stable plugin-scoped key, an approved scene and its own registered SELF origin. Its execution has no human source or borrowed actor. Replaying the key retrieves the original proposal. A changed body or scene conflicts. Paused groups and unavailable receivers prevent admission and later dispatch.

Source pages expose bounded previews with actual IDs. Detailed source reads use bounded byte pages and acquire the plugin's erasure obligation before returning content. Auxiliary reviews declare those source IDs through the auxiliary model interface. Feedback derives confirmed, failed, unknown and quiet outcomes from the original execution and physical send records.

Request `include_origin: true` on `participation.source` for the original entry,
raw Person and account assignment. The same scoped read also reports the
current Person root and conversation generation. Compare that generation with
`observed_generation`, captured by the ingress writer when it first stored the
event. A pre-migration observation has `observed_generation: null`. The original
envelope bytes and their version are unchanged. For a live continuation, pass
the observed value as `expected_generation` to `participation.promote`. The
writer requires it to match both the stored observation and the current
conversation generation before promotion.

Request `include_reply: true` for an internal `reply_to` event and its source
version. The original locator must identify a single earlier human message in
the same group entry, receiving assignment and recorded generation. A missing
or erased anchor, unknown generation, SELF echo or ambiguous locator yields
`null`. The writer retains the quoted source on the plugin's erasure ledger
before returning the reference. The reply projection does not alter the focus
envelope or authorize promotion by itself.

Send `mention_cursor: 0` to read the first page of original native annotations.
Each page holds up to 16 items in ingress order, including repeated targets.
Use `mentions.next` as the next mention cursor and keep the source `version`.
The byte cursor is independent. A recorded human account transfer does not
replace the original assignment or Person. Unresolved identity and unrecorded
offsets are null. Recorded offsets use Unicode scalar positions in the
original rendered text. Literal at-sign text has no native annotation.

If a mentioned Person is forgotten, participation stops serving the source
and the Host asks previous participation owners to erase their copies by
event ID. The original author's event remains in the ledger. Derived
participation requests tied to that source lose their bodies and retain
accounting and effect receipts.

Acceptance races rule and invitation promotion, transfer or disable the original actor, erase a hydrated source, restart without the original envelope, pause a group before activation, and exercise a real SELF send after another member's message. The receiver, original execution, source IDs and model request counts are recorded in `promotion-vs-self.json`.

The boot configuration records available QQ receivers and configured groups in the writer. An authenticated recovered group retains its explicit receiver. Removing QQ configuration prevents new participation admissions. External SELF delivery uses the installed gateway matching the original receiver and records each actual part in the common outbound table. An unknown physical part remains unknown after cold restart.

Candidate schema 1 uses Rust SDK `1.0.0-alpha.28` and Python `1.0.0a28`. True EXT-025 consumption is pending. The core provides no sampling or Jev algorithm.
