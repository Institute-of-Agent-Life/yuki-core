# Replay the fixed v4 oracle

Use Python 3.12 to run the portable comparison in a core checkout.

```sh
python ci/check.py CORE-002 --out artifacts/CORE-002/01
```

The output contains the two-capture comparison, deliberate delivery and signature
failures, workload observations, the unpublished consumer ZIP, and its SHA-256.
EXT-007 and EXT-019 can verify the ZIP SHA-256, read `candidate.json`, check
each member against its digest, then read `protocol/baseline-v4/`. The connector
transcript covers the observed reconnect scenario. The Rust plugin SDK has no
version in this candidate.

## Regenerate the observations

Use an isolated checkout of `329ca7c487414d2f54ddd98ff4727446ed2446cd`
and its Python 3.12 environment. Install the frozen `uv.lock` dependencies,
build the original vec1 library for the selected platform, and install the three
capture-only versions in `baseline/lock.json`. The interpreter's SQLite must
actually support FTS5 with contentless deletion. On Windows the capture used
SQLite 3.46.0 and an isolated vec1 DLL. Provision dependencies before replay.

Set `PYTHONHASHSEED=329`, `PYTHONUTF8=1` and a loopback-only `NO_PROXY`.
Run each command with an empty provider credential environment. All providers
and QQ accounts in these fixtures are synthetic. Use the same previously unused
scratch pathname for each successive capture. The recorder creates and removes
its own scratch directory. The two runs use separate output directories.

```sh
"$V4_PYTHON" baseline/replay/record.py --source "$V4" --out "$PAIR/first" --scratch "$SCRATCH"
"$V4_PYTHON" baseline/replay/record.py --source "$V4" --out "$PAIR/second" --scratch "$SCRATCH"
"$V4_PYTHON" baseline/replay/load.py --source "$V4" --out "$LOAD"
python baseline/replay/freeze.py --source "$V4" --pair "$PAIR" --workload "$LOAD" --destination "$NEW_EXPORT"
```

The A schedule shuffles 200 private and 1,000 group inputs with seed 329 and
waits for each turn to finish. It records the actual gateway call and SQLite
event for every visible reply. Another 20 independent fixtures traverse device
pairing, a QQ Work, fresh runtimes and final file delivery. The Compact probe
captures execution Compact, public Compact and the subsequent ordinary request.
The measured process CPU and elapsed time apply to this offline A run only.
The saved B configuration equals A. C awaits a feature-equivalent Rust host.

`docs/taskbook/v3.2/audit/results/` separately identifies the older upstream
CI archives and the digests of their source snapshots. The oracle index
preserves the older CI SHA alongside the current capture SHA.
