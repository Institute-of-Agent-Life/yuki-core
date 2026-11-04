"""Original-author retention, flattened merges, two-ended execution authority and migration."""
import sqlite3
from contextlib import closing
from tests.harness.service import Service, previous_schema


def run(ctx):
    data = ctx.working / "upgrade"
    previous_schema(ctx, data, 1)
    with closing(sqlite3.connect(data / "core.sqlite")) as db:
        db.execute("INSERT INTO core_meta VALUES('label','old store',3)")
        db.commit()
    owner = Service(ctx, data)
    upgrade = owner.command("migrate")
    ctx.check("v1 upgrades to identity schema", upgrade["schema_version"] >= 2)
    a,b,c,d = [f"00000000-0000-4000-8000-{i:012x}" for i in range(1,5)]
    device = "00000000-0000-4000-8000-000000000010"
    def person(*args, **kwargs):
        return owner.command("operator", "person", *args, **kwargs)
    with owner.running():
        ctx.equal("v1 label preserved", owner.command("operator","get-label"), {"value":"old store","revision":3})
        info = owner.command("operator", "database-info")
        ctx.write("sqlite-build.json", info)
        for actor in (a,b,c,d):
            person("create", actor, "--name", "same name")
        ctx.equal("same name does not merge", person("get", d)["family"], [d])
        person("bind-device", device, a)
        before = person("check-device", device)
        person("set-admin", a, "true")
        person("merge", a, b)
        ctx.check("root inherits management", person("check-device", device)["admin"])
        person("set-admin", b, "false")
        person("merge", a, c)
        after = person("get", a)
        ctx.equal("A and B flattened to C", after["family"], [a,b,c])
        second_device = "00000000-0000-4000-8000-000000000011"
        person("bind-device", second_device, a)
        ctx.equal("new device after merge also retains original", person("check-device",second_device)["original"], a)
        ctx.equal("historical original survives", (after["original"],after["root"]), (a,c))
        ctx.check("revoked administration not restored by another merge", not person("check-device", device)["admin"])
        ctx.equal("reverse same-family merge does not redirect", person("merge", c, a)["root"], c)
        person("merge", c, "00000000-0000-4000-8000-000000000099", accepted=False)
        ctx.equal("failed merge rolls back", person("get", a)["root"], c)
        person("set-admin", c, "true")
        person("set-enabled", a, "false")
        denied = person("check-device", device, accepted=False)
        ctx.check("disabled original cannot borrow root", "disabled" in denied)
        ctx.check("accountless C can still administer", person("get", c)["admin"])
        person("bind-device", device, c, accepted=False)
        person("set-enabled", a, "true")
        person("set-enabled", c, "false")
        ctx.check("disabled root blocks enabled original", "disabled" in person("check-device", device, accepted=False))
        person("set-enabled", c, "true")
        final = person("check-device", device)
        ctx.equal("device retains its source even after re-enable", (final["original"],final["root"]), (a,c))
    with owner.running():
        ctx.equal("family persists across restart", person("get", a)["family"], [a,b,c])
        ctx.equal("device retains its source across restart", person("check-device", device), final)
    return {"before_device":before,"after_merge":after,"after_device":final,"disabled_error":denied,
            "previous_schema":1,"upgraded_schema":info["schema_version"]}
