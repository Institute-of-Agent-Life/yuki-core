# Host candidate

The CORE-032 candidate contains the tested native host, Rust and Python SDK distributions, both schemas and a per-file digest manifest. The host advertises business schema revision 2 separately from handshake revision 1. Development builds use Rust SDK `1.0.0-alpha.2` and Python SDK `1.0.0a2`.

Install an executable through a `[[plugins]]` entry in the owner's private `yuki.toml`. Specify `id`, `namespace`, an absolute `program` and `args`. A gateway also needs its `platform` and the installed SELF assignment's `bot_stage`. Configure `grants` individually. `commands` lists full command paths. `[[plugins.tools]]` records the name, description, JSON Schema and visibility that the host freezes for the next activation. The `env` list selects which environment variables reach this executable.

The child reads its connection binding from `YUKI_PLUGIN_BINDING`, then initiates `initialize`. The reply contains the negotiated `method_revisions` and `host_schema_revision`. Python's `yuki_plugin_sdk.host.Calls` and Rust's `yuki_plugin_sdk::host::Calls` use that reply to pair requests. Keep reading stdin while awaiting a callback. Incoming and outgoing requests can use the same ID. Business methods and their directions are defined in `host-preview.json`.

A gateway submits platform-native identifiers and segments. Its configured namespace and SELF stage select its authority. Store an input cursor only after the durable receipt. A source's lookup key comprises the original sender, source ID, namespace-bound bot, space, topic and optional native revision. For sending, the host supplies the same source route to a pure plan and then dispatches each immutable part in order. If the connection fails after dispatch, reconcile the original part handle. The host preserves the confirmed prefix.

Metadata observation is instance-bound and may drop under backpressure. Forget uses the durable owner, watermark and key. Cleanup acknowledges the original obligation even when its first acknowledgement was lost. The first candidate contains no approved third-party QQ native action. A maintainer must supply the actual action and scope before that method can be included.

Run `python ci/check.py CORE-032 --out <new-directory>` to exercise the host and install both SDKs. Run `python ci/host_candidate.py --manifest <verified-manifest> --out <new-directory>` on the same clean checkout to package the tested binary. The manifest records its build profile, platform, source SHA, schema and SDK versions, verification digest and contained methods.
