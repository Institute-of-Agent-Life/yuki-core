# Retained usage projection

`model.stats` and `GET /admin/v1/models/stats` share the current original-person
and operator authorization from CORE-037. `usage.json` pins the existing Admin
DTO for EXT-049; schema revision 1 is an unpublished development contract.
Rust SDK `1.0.0-alpha.11` and Python SDK `1.0.0a11` already carry `control.read`.
Real EXT-049 consumption remains pending.

The time window is start-inclusive and end-exclusive over each saved physical
dispatch. `today` uses the chosen IANA zone's calendar midnight; `24h` and `7d`
are rolling intervals. `all` includes only retained rows, with no lower bound;
it does not reconstruct erased or never-recorded billing history. Task,
provider, model and profile labels come from the original frozen dimensions.

The existing invocation, success, failure and coverage counters count logical
calls. Known token sums include only reported fields of physical attempts in
the selected window, including literal zero. Latency retains the original
logical call's observed endpoints. In-flight calls remain excluded. Cache
numerator and denominator include only attempts reporting both fields; the
reported-call count includes each logical call with such a pair once. An
input-only attempt and a later cache-only attempt never form a pair. Cache
creation totals and 5-minute/1-hour fields stay separate.

Missing pricing remains unknown in the original billing ledger. This projection
creates no price estimate, usage writer, physical request or retry permission.
The test fixture retains the upstream six-call 47/110 result and its original
SHA-256; a separate real loopback retry checks split fields and midnight.
