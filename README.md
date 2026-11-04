# yuki-core

Rust core rewrite, implemented card by card against the fixed Yuki baseline
`329ca7c487414d2f54ddd98ff4727446ed2446cd`.

The first executable component is the offline replay harness. Python 3.12 and
the standard library are sufficient:

```text
python ci/check.py CORE-001 --out artifacts/CORE-001/01
```

From the development Mac, run the same source on the authorized Windows SSH host:

```text
python3 ci/windows.py CORE-001 --host hjw --out artifacts/CORE-001/windows-01
```

Each attempt writes a manifest and named evidence. Output directories are never
reused. The Windows helper verifies the snapshot's SHA-256 and every source file,
then retrieves the original evidence. See [evidence](docs/evidence.md),
[implementation progress](docs/implementation-progress.md), and the
[taskbook](docs/taskbook/v3.2/Yuki-Rust重寫總任務書-v3.2.md).

No runtime, SDK, stable protocol, or release is claimed by the harness bootstrap.
