"""An executable can remain non-deletable after the clean-PATH probe succeeds."""
import os,threading
from pathlib import Path

def check_workspace(ctx):
    from host_candidate import cleanup_workspace,portable_workspace
    attempts=[];locked=PermissionError('synthetic Windows sharing violation');locked.winerror=32
    def transient():
        attempts.append(1)
        if len(attempts)==1:raise locked
    cleanup_workspace(transient)
    ctx.equal('workspace cleanup retries a transient sharing violation',len(attempts),2)
    def persistent():raise locked
    ctx.rejects('persistent sharing violation remains a failure',PermissionError,lambda:cleanup_workspace(persistent,timeout=0))
    def unrelated():raise PermissionError('unrelated permission failure')
    ctx.rejects('unrelated permission error is never retried',PermissionError,lambda:cleanup_workspace(unrelated))
    if os.name!='nt':return
    with portable_workspace() as d:
        path=Path(d)/'locked-probe.exe';path.write_bytes(b'file-lock fixture')
        with path.open('rb') as held:
            try:path.unlink()
            except PermissionError as error:ctx.equal('real Windows file handle denies deletion',error.winerror,32)
            else:raise AssertionError('fixture did not lock deletion')
            # Keep a second non-delete-sharing handle alive across context exit.
            remaining=path.open('rb')
            release=threading.Timer(0.4,remaining.close);release.start()
    release.join()
    ctx.check('candidate cleanup completes after real lock release',not Path(d).exists() and remaining.closed)
