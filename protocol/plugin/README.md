# Plugin handshake preview

The only method in this candidate is `initialize`. The schema defines its
version range, required and optional features, fixed manifest declarations,
method revision and selected extension points. Schema revision 1 proposes
wire 1.0. SDKs are prereleases. The runtime method index and the negotiated
extension point list stay empty of business methods until their own cards
supply real consumers.

Each stdio line carries one JSON-RPC 2.0 object, up to 65,536 UTF-8 bytes
before the newline. IDs are strings or signed 64-bit integers. A connection
can initiate a request while responding to another with the same numeric ID.
An error replies to its original ID. Outstanding requests fail on EOF. On a
fresh connection a reused numeric ID has no relation to the previous peer.

The receiving host supplies the trusted namespace, narrow grants, manifest
revision and declarations. A remote offer cannot replace them. An unknown
required feature, changed manifest, duplicate declaration, incompatible
version or undeclared method produces a scoped error. Unknown optional fields
are ignored. The conformance programs accept `--binding <file>` for a trusted
test manifest and `--initiate` to start a simultaneous handshake.

## Build and consume

```sh
python protocol/plugin/generate.py --check
python ci/check.py CORE-031 --out artifacts/CORE-031/local
python ci/publish-contract.py verify artifacts/CORE-031/local/handshake-preview
```

Verify the ZIP SHA-256 against the producer's handoff record. Extract its
members into a new directory. Preserve the ZIP there under its original name
for the pin command. The lock is replaced only after all artifact checks pass.

```sh
python ci/publish-contract.py pin "$DIST" --expected-sha256 "$EXPECTED_SHA" --lock "$NEW_LOCK"
python -m pip install --no-index --no-deps "$DIST"/*.whl
python -m yuki_plugin_sdk.peer
python "$DIST"/yuki-conformance-peer.pyz
```

The Rust distribution is a Cargo crate. Extract it into a clean directory,
then run `cargo build --locked --offline --bin yuki-plugin-peer` after
provisioning its locked dependencies. The Rust library exports `Peer`,
`Binding`, the schema-generated `Initialize`/`Initialized` models and the
line reader. The Python library exposes the equivalent types and framing
helpers. The candidate manifest identifies each archive, schema, method
index, golden, portable peer and the installed crosswire transcript by hash.

The `build`, `verify` and `pin` commands operate locally. A preview built
from a dirty tree is identified in the manifest and requires a development
override to pin. CORE-031 exercises only the handshake. B0 promotion still
depends on the host and actual extension consumption.
