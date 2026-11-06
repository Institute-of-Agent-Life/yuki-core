# Management candidate

Unpublished preview: handshake 1, Host 2, control methods 1, Rust SDK
`1.0.0-alpha.11`, Python SDK `1.0.0a11`. Real extension acceptance remains
pending EXT-022, EXT-037, EXT-038, EXT-039 and EXT-049.

`control.read` and `control.write` require `control.invoke` plus an original
`command.invoke` or `tool.call` handle. The core resolves the authenticated
plugin, current generation, original person and current capability on every
call. A supplied actor or role is rejected. A superuser device never receives
operator-only capabilities. The same service backs local OS operator commands
and `/admin/v1`; `protocol/baseline-v4/admin-openapi.json` retains the HTTP DTOs.

| Read actions | Write actions | Capability family |
|---|---|---|
| `character.public`, `character.identity` | — | `control.preference.read` |
| `character.get`, `character.soul.read` | `character.update`, `character.soul.save`, `character.soul.reload` | `control.character.*` |
| `plugin.list`, `plugin.get`, `plugin.outbox.get` | `plugin.enable`, `plugin.disable`, `plugin.approve`, `plugin.doctor`, `plugin.discover`, `plugin.outbox.retry` | `control.plugin.read` / `control.plugin.mutate` |
| `plugin.configuration.read`, `plugin.observe` | `plugin.configuration.save` | `control.plugin.config.content.read` / `control.plugin.config.mutate` |
| `work.list`, `work.get` | `work.cancel`, `work.resume` | `control.work.*` |
| `person.list`, `person.get`, `binding.list`, `binding.get`, `presence.list`, `presence.get` | Person, binding, device and presence actions in the fixed Admin contract | Action-specific `identity.*` |
| `model.read`, `model.stats` | `model.apply`, `model.secret`, `model.test`, `model.discover` | Operator model configuration capabilities |
| `health.read`, `operation.get` | — | `control.health.read`, `control.operation.read` |
| `trace.read` | — | `control.execution.metadata.read`; content additionally requires `control.execution.content.read` |

A write body uses the fixed DTO with `request_id` and, where required,
`expected_revision`. Resource actions add `id`; plugin actions add `plugin_id`.
Request IDs are scoped to the authenticated issuer, cannot change methods or
bodies, and return their original receipt. `operation.get` exposes completion
metadata without staged bodies or secrets. Model probing is an explicit one-shot
request with no Work, paid journal, automatic retry or credential persistence.

An admitted deterministic command failure keeps its original failed receipt
after rolling back partial mutation. Replaying that ID returns the same failure
even if the resource later changes; a corrected command needs a new ID. Current
authority is checked before replay or receipt reads. Invalid DTOs and denied
authority are rejected before admission and do not consume a request ID.

`character.identity` returns the permanent role ID, current name, revision and
deduplicated current-name/deployment aliases. SOUL replacement uses a prepared
receipt, atomic file rename and activation. An already paid continuation keeps
its original role and SOUL; a later activation sees the new revision.

Plugin discovery reloads local declarations without launching new or changed
programs. An altered declaration requires fresh approval; requested grants and
approved grants are separate. Configuration is readable and writable against the
approved definition while stopped, before first enable, and after start failure.
Secret actions are explicit keep/replace/clear. Observation requires a live
instance and rechecks generation and authority before publication.

`plugin.outbox.get` takes a decimal local outbox `id`. It reports original
publication/ordinal, revision, state, attempts, limit and public error category.
The fixed HTTP retry route is `POST /admin/v1/plugin-outbox/{id}/retry`.
Only a safe predispatch failure with an untouched original effect can re-enter
the queue. Confirmed, unknown, exhausted, erased or revoked deliveries are denied.
Requeue never creates another publication, source, execution, root budget or
physical effect ID. Local outbox IDs are additive; historical import must bind
legacy IDs explicitly rather than infer an equivalence.

Trace is an authorized bounded projection, not a completion receipt. Secret
fields and private encoded objects are removed recursively. Work management
rechecks the original execution, source, route and generation; an operator
cannot grant an unknown paid request another attempt.
