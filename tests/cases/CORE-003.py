"""A quiesced WAL snapshot, a missing referenced blob, and an absent deployment."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

def run(ctx):
    snapshot = ctx.working / "snapshot"
    builder = ctx.root / "fixtures/v4-0156/create.py"
    subprocess.run([sys.executable, str(builder), str(snapshot)], check=True)
    inventory = ctx.root / "baseline/inventory.py"
    def collect(source, out):
        return subprocess.run([sys.executable, str(inventory), "--snapshot", str(source),
                               "--out", str(out)], capture_output=True)
    def hashes():
        return {p.relative_to(snapshot).as_posix(): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in snapshot.rglob("*") if p.is_file() and not p.name.startswith("._")}
    before = hashes()
    path = ctx.working / "inventory.json"
    result = collect(snapshot, path)
    ctx.equal("read-only inventory completes", (result.returncode, result.stderr), (0, b""))
    doc = json.loads(path.read_bytes())
    ctx.equal("all source bytes unchanged including WAL", hashes(), before)
    rows = doc["rows"]
    def one(owner, kind):
        entry, = (r for r in rows if r["owner"] == owner and r["original_class"] == kind)
        return entry
    ctx.equal("0156 comes from SQLite", doc["versions"]["database"], "0156")
    ctx.equal("0.4.3 comes from installed METADATA", doc["versions"]["connector_distribution"], "0.4.3")
    ctx.check("image identified independently", doc["versions"]["image_digest"].startswith("sha256:"))
    ctx.equal("disagreeing historical descriptions retained", doc["versions"]["documents"], {"changelog":"0143","readme_schema":"0155","readme_connector":"0.4.2"})
    ctx.equal("WAL-only original Work visible", one("core", "runtime_work")["count"], 1)
    ctx.equal("unassigned original person remains", one("core", "persons_without_account")["count"], 1)
    ctx.equal("forgotten person absent from source rows", one("core", "persons")["count"], 2)
    ctx.equal("forget watermark retained", doc["privacy_watermark"], "forget:7")
    blobs = [r for r in rows if r["original_class"] == "artifact_bytes"]
    ctx.equal("missing bytes distinct from empty", sorted(r["status"] for r in blobs), ["missing","observed"])
    ctx.check("missing blob has no target", all(r["target_ref"] is None for r in blobs))
    ctx.check("owner coverage", {"core","workspace","participation","plugin:fixture","manager","connector",
                                   "media","protocol","credentials"} <= {r["owner"] for r in rows})
    ctx.check("no secret value or forgotten content", "synthetic-secret-do-not-read" not in json.dumps(doc))
    ctx.check("no effects dispatched", all(r["effect_delta"] == 0 for r in rows))
    ctx.equal("unauthorized deployment awaits sampling", doc["deployment"]["status"], "awaiting_authorized_snapshot")
    ctx.equal("image corroborates deployment receipt", doc["versions"]["image_binding"], "matched")
    meta = snapshot / "snapshot.json"
    settings = json.loads(meta.read_bytes())
    byte_source = meta.read_bytes()
    settings["databases"]["core"]["path"] = "../outside.db"
    meta.write_text(json.dumps(settings))
    ctx.check("snapshot path escape rejected", collect(snapshot, ctx.working / "escape.json").returncode != 0)
    meta.write_bytes(byte_source)
    # An otherwise valid main DB without its frozen WAL must not pass.
    (snapshot / "core.db-wal").unlink()
    ctx.check("lost WAL rejected", collect(snapshot, ctx.working / "broken.json").returncode != 0)
    ctx.check("failed capture publishes no report", not (ctx.working / "broken.json").exists())
    return doc
