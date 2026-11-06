# Offline model configuration import

`yuki-core import-models` converts a stopped **copy** of a v4 schema 0156 deployment into a new model bundle. It starts no core owner, gateway, plugin or provider. The source TOML, sidecar and `.env` remain unchanged. Only the original legacy agent-default SQL rows are retired after the new bundle has been read back successfully. Unrelated tables and effects remain in the source copy.

Choose the actual selected TOML, its adjacent `model_profiles.secrets.json`, the stopped SQLite copy including its original WAL, and an explicit JSON capture of process environment values. Pass the actual `.env` path separately. The capture overrides `.env`; the importer never takes provider settings from its own ambient environment. Receipts list source paths, digests, field names and SQL identities, without secret values. Complex dotenv interpolation is rejected so it cannot silently consume another process's environment.

```sh
yuki-core --data /new/location/bundle import-models --profile /stopped/selected.toml --database /stopped/core.db --environment /stopped/process-environment.json --dotenv /stopped/.env --stopped-copy --inspect
yuki-core --data /new/location/bundle import-models --profile /stopped/selected.toml --database /stopped/core.db --environment /stopped/process-environment.json --dotenv /stopped/.env --stopped-copy --revision ORIGINAL_REVISION
```

Use the revision returned by `--inspect`. The destination must not exist on first apply; its parent must exist. The legacy Application OS lock must be available. The SQL schema must be 0156; another source version is rejected before destination publication.

Explicit null legacy nonnullable rows fall back to their original environment/default source. Explicit false remains false. Existing connection sections take precedence; absent sections use the original effective settings. A Tavily key does not enable search. A shared Profile is split only when agent defaults differ; other purposes, private headers, unknown operator fields and comments remain intact. Each purpose is compared again after producing the actual TOML bytes.

If publication succeeds but SQL retirement is interrupted, keep the destination and run the identical command with `--resume` and the **original source revision**. The importer reads its original intent, verifies the actual bundle and source hashes, then compares every retained row identity, key, scope, value digest and version. It never rebuilds a changed destination. Mixed, changed or unconfirmed SQL rows produce `partial` with original paths; an intact already-retired set settles by readback. SQL deletion is one transaction, so a failure rolls back all its deletions.

The bundle is an offline candidate. Loading it into a deployment and endpoint availability require separate validation. This command does not change arbitrary `.env` files or launch any endpoint probes. `YUKI_TEST_MODEL_IMPORT_BARRIER` is an opt-in local fault-test barrier; normal invocations do not set it.
