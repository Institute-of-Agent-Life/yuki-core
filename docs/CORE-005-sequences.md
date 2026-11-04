# CORE-005 sequences

Normal: init creates one config and SOUL; duplicate init preserves both. Migration
holds the data-directory OS lock. Run acquires that same lock, verifies schema,
then serves live/ready/health and owner-only local control. QQ need not be online.
A label mutation goes through the service writer. Health and default doctor
perform no mutation or external request. Shutdown closes the writer before
releasing the OS lock; offline migration can then acquire it.

Faults defined before implementation:

1. A second service and offline migration must fail while the first owns the directory.
2. A canceled caller must not release its writer while rollback or physical close is unresolved.
3. Inject rollback failure and block real connection close: SELECT 1 on a separate
   reader remains possible, writer_accepting is false, and the next mutation stays queued.
4. Confirm close, reopen the writer, and finish the queued mutation without preserving
   the rolled-back value. Unknown close instead stops admission until restart.
5. Emit one hint for a confirmed root commit after releasing the active writer;
   rollback and savepoint release alone emit zero hints.

The process/IPC/HTTP sequence runs through `tests/cases/CORE-005.py`. The injected
database-driver cleanup boundary uses Rust tests against real SQLite and records
its output separately. It is not labeled QQ or whole-product E2E acceptance.
