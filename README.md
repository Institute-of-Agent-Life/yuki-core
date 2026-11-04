# yuki-core

Rust core rewrite, implemented card by card against the fixed Yuki baseline
`329ca7c487414d2f54ddd98ff4727446ed2446cd`.

The Rust executable provides data-directory ownership, additive SQLite migration,
live/ready/health endpoints, and private local operator commands. Rust 1.96.0 is
pinned in `rust-toolchain.toml`; SQLite has foreign keys, WAL and FTS5 enabled.

```text
cargo fetch --locked
cargo build --locked --offline
target/debug/yuki-core --data ./data init
target/debug/yuki-core --data ./data run
```

`init` creates a config and copies the baseline persona into the data directory as
an editable `SOUL.md`. Existing files are preserved. An empty SOUL supplies
only the character's name. `run`, `migrate` and `doctor` read the existing configuration.
A configured `[main]` section runs Chat Completions on accepted inputs. For a local
HTTP peer, add these settings to the data directory's `yuki.toml`.

```toml
[main]
endpoint = "http://127.0.0.1:8081/v1/chat/completions"
model = "local-chat"
name = "Yuki"
merge_ms = 450
root_slots = 6
max_input_units = 100000
max_output = 128
```

`root_slots` bounds logical model requests. The shared unit ceiling covers
estimated input plus the maximum output for each reserved request. Main uses the
UTF-8 request length as a conservative input estimate. Recorded provider usage
and unknown charges remain on their physical attempts. A remote profile requires
an explicit HTTPS endpoint and `allow_remote = true`. An optional `key_env` names
the environment variable containing its credential.

Set `protocol` to `openai_responses`, `deepseek_responses_legacy`, or
`deepseek_responses` with the corresponding Responses endpoint to replay ordered
items locally. The legacy DeepSeek adapter omits `tool_choice`. The documented
DeepSeek dialect can express a named function. The OpenAI dialect requests encrypted
reasoning with storage disabled. The offline comparison records the native HTTP
bodies in `responses-dialect.json`. The baseline and official references are in
`fixtures/providers/responses/`.

The operator uses a mode-0600 Unix socket or a Windows named pipe whose protected
DACL permits the current OS user and SYSTEM. For example:

```text
target/debug/yuki-core --data ./data operator database-info
target/debug/yuki-core --data ./data operator set-label "local instance"
target/debug/yuki-core --data ./data operator shutdown
```

The Python 3.12 verifier drives the actual Rust binary and records evidence:

```text
python ci/check.py CORE-001 --out artifacts/CORE-001/01
python ci/check.py CORE-005 --out artifacts/CORE-005/01
```

From the development Mac, run the same source on the authorized Windows SSH host:

```text
python3 ci/windows.py CORE-001 --host hjw --out artifacts/CORE-001/windows-01
python3 ci/windows.py CORE-005 --host hjw --out artifacts/CORE-005/windows-01
```

Each attempt writes a manifest and named evidence. Output directories are never
reused. The Windows helper verifies the snapshot's SHA-256 and every source file,
then retrieves the original evidence. See [evidence](docs/evidence.md),
[implementation progress](docs/implementation-progress.md), and the
[taskbook](docs/taskbook/v3.2/Yuki-Rust重寫總任務書-v3.2.md).

The verifier builds with the lockfile and offline cache, runs Rust cleanup-boundary
tests, then checks the service through real HTTP, local IPC and SQLite. CI covers
Windows, macOS and Linux. Current completion and candidate status are recorded in
the progress document.

The private operator can create accountless persons, merge them, and manage
their current enablement and administrative grants. For example,
`yuki-core --data ./data operator person get <original-uuid>` shows the original
ID, current root and family. Device origins remain bound to the original ID.
Schema 2 adds `persons` and `device_origins` to a schema 1 database through the
numbered migration. Reader version 2 understands both tables.

Schema 3 records account assignments in their original platform and tenant/app
namespace. The `operator account` commands open, pause, transfer and resolve
stages. Their integer timestamps are UTC milliseconds. A proven occurrence
time selects a half-open ownership interval. Ambiguous undated input returns
only the account. `operator entry` creates private and group entries, and
checks current private read/send eligibility. Transferring either endpoint
closes its old private entry while retaining its original key and history.

Schema 4 separates observed events from accepted sources. The protected
`operator ingress record` command takes a normalized JSON frame. `--accept`
requests ordinary admission after the writer rechecks the original actor,
receiving bot, scope, and segment type. `operator ingress pending` lists
accepted sources without an execution. `operator ingress claim <event>`
returns the same execution on retry, together with the retained envelope
and ordered native mentions after checking current authority.

The stdio handshake preview ships an independently installable Rust crate,
Python wheel, JSON Schema, method index and portable conformance peer. Build
and compare it with `python ci/check.py CORE-031 --out artifacts/CORE-031/local`.
See the [handshake contract](protocol/plugin/README.md).

Schema 6 binds each model activation to its execution and frozen profile
contract. The root reserves one request slot and the input estimate plus
the output allowance in the transaction admitting a logical request.
Reservations remain conservative across physical retries, cancellation and
Work attachment. Actual usage and its field-level coverage are reported
separately from those estimates. A missing charge stays unknown.

The `operator model` boundary persists the original request bytes, grants a
single physical dispatch, saves the returned body and ordered tool intents
before pairing them, and recovers the original receipt on restart. A fresh
owner treats dispatched attempts without a response as unknown. Only an
explicit one-retry policy with a documented unbilled pre-model rejection can
grant another attempt on the same logical request. Cancellation or an unknown
attempt cannot create that permission.
