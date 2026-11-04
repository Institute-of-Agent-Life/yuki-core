"""Start the real Rust owner with private local IPC and a disposable data directory."""
import json
import os
import subprocess
from contextlib import contextmanager

class Service:
    def __init__(self, ctx, data):
        self.ctx = ctx
        self.data = data
        self.binary = ctx.root / "target/debug" / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
        self.argv = [str(self.binary), "--data", str(data)]

    def command(self, *args, accepted=True):
        result = subprocess.run([*self.argv, *args], capture_output=True, text=True, timeout=20)
        if accepted and result.returncode:
            self.ctx.write("failed-command.json", {"args":args,"stderr":result.stderr,"exit_code":result.returncode})
        self.ctx.equal("command " + " ".join(args) + (" succeeds" if accepted else " rejects"),
                       (result.returncode == 0, result.stderr if accepted else ""), (accepted, ""))
        return json.loads(result.stdout) if accepted else result.stderr.strip()

    @contextmanager
    def running(self):
        server = subprocess.Popen([*self.argv, "run"], stdout=subprocess.PIPE, text=True)
        try:
            self.ctx.equal("owner listening", json.loads(server.stdout.readline())["status"], "ready")
            yield self
            self.command("operator", "shutdown")
            self.ctx.equal("owner shutdown", server.wait(timeout=10), 0)
        finally:
            if server.poll() is None:
                server.kill()
                server.wait(timeout=10)
            server.stdout.close()
