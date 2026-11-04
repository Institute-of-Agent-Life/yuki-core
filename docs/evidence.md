# Reproducible evidence

Run a card with Python 3.12:

```text
python ci/check.py CORE-001 --out artifacts/CORE-001/01
```

Every attempt needs a new output directory. Failed runs retain a manifest, the
exit code, the failure, and any observations recorded before the failure. Empty
cases cannot pass. Artifacts contain synthetic data; credentials are excluded
from the case process environment. The initial harness has no package dependencies.

CORE-001 first defines this sequence: send synthetic event 001, record it before
acknowledging it, then repeat for 002. The client and HTTP recorder must agree on
both responses and the independent fixture order. Swapping the two events must
fail `event order`. Returning without assertions must fail `empty case`. A second
normal replay must produce identical bodies and virtual timestamps. External IP
and DNS requests must be refused by the Python socket boundary.

This boundary controls Python networking, not arbitrary native subprocesses.
Later cards must identify the network controls used for their native processes.
Loopback HTTP evidence is not QQ delivery or a paid-provider acceptance result.

The source manifest records the branch, commit (null for the initial unborn
checkout), working-tree state, source file hashes, Python/SQLite versions, seed,
clock, commands, exit status, and hashes of produced files. A Windows source
snapshot is verified against its file hashes before tests execute.

The original taskbook ZIP remains unchanged. The working taskbook carries the
approved K05 correction to CORE-011 step 2; its audit files remain the original
document-review evidence and are not new product-test results.
