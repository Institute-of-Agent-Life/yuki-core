"""Dated ownership and DM isolation through the running Rust owner."""
from tests.harness.service import Service, previous_schema
import sqlite3
from contextlib import closing

def run(ctx):
    owner = Service(ctx, ctx.working / "data")
    previous_schema(ctx, owner.data, 2)
    with closing(sqlite3.connect(owner.data/"core.sqlite")) as db:
        db.execute("INSERT INTO persons(id,name) VALUES(?,?)",("00000000-0000-4000-8000-000000000099","v2-original"))
        db.commit()
    ctx.check("upgrade from v2", owner.command("migrate")["schema_version"] >= 3)
    a,b = [f"00000000-0000-4000-8000-{i:012x}" for i in (1,2)]
    with owner.running():
        ctx.write("sqlite-build.json", owner.command("operator","database-info"))
        ctx.equal("preexisting v2 identity preserved",owner.command("operator","person","get","00000000-0000-4000-8000-000000000099")["name"],"v2-original")
        for actor in (a,b):
            owner.command("operator","person","create",actor)
        def account(*args, **kw):
            return owner.command("operator","account",*map(str,args),**kw)
        def entry(*args, **kw):
            return owner.command("operator","entry",*map(str,args),**kw)
        first = account("open","qq","tenant-A","123",10,"--person",a,"--start",10)
        other = account("open","qq","tenant-B","123",10,"--person",b,"--start",10)
        ctx.check("same native ID in two namespaces stays separate", first["account"] != other["account"])
        bot = account("open","qq","tenant-A","900",9,"--start",9)
        stage, bot_stage = first["assignment"], bot["assignment"]
        entry("private",other["assignment"],bot_stage,accepted=False)
        dm = entry("private",stage,bot_stage)["entry"]
        ctx.equal("NULL topic de-duplicates", entry("private",stage,bot_stage)["entry"], dm)
        threaded = entry("private",stage,bot_stage,"--topic","x")["entry"]
        ctx.check("present topic is a distinct entry",threaded != dm)
        group = entry("group","existing-space")["entry"]
        ctx.equal("group entry by original space",entry("group","existing-space")["entry"],group)
        account("enabled",stage,"false",15)
        entry("private",stage,bot_stage,accepted=False)
        account("open","qq","tenant-A","123",20,"--person",b,accepted=False)
        account("enabled",stage,"true",25)
        paused = account("resolve",first["account"],28,"--occurred",18)
        ctx.equal("disabled interval retains owner without execution authority", (paused["original"],paused["status_at_source"]),(a,"disabled"))
        account("resolve","no-such-account",29,accepted=False)
        second = account("transfer",stage,30,31,"--person",b)
        ctx.equal("transfer retry returns same new stage",account("transfer",stage,30,31,"--person",b), second)
        native = account("resolve",first["account"],35,"--occurred",29)
        ctx.equal("late but proven source resolves previous owner", (native["assignment"],native["original"]),(stage,a))
        unclear = account("resolve",first["account"],36)
        ctx.equal("missing native time stays account-only", (unclear["assignment"],unclear["original"]),(None,None))
        ctx.equal("known transfer boundary belongs to new stage",account("resolve",first["account"],36,"--occurred",30)["original"],b)
        ctx.equal("old DM unavailable to new holder",entry("access",dm,b)["read"],False)
        ctx.equal("old holder may read but cannot dispatch",entry("access",dm,a),{"read":True,"send":False})
        ctx.equal("threaded old DM closes too",entry("access",threaded,a)["send"],False)
        next_dm = entry("private",second["assignment"],bot_stage)["entry"]
        ctx.check("new holder has independent DM",next_dm != dm)
        third = account("transfer",second["assignment"],40,40,"--person",a)
        ctx.check("transfer back creates a third stage",third["assignment"] not in (stage,second["assignment"]))
        fresh = entry("private",third["assignment"],bot_stage)["entry"]
        ctx.check("transfer back does not reopen original DM", fresh not in (dm,next_dm))
        account("transfer",stage,30,55,"--person",a,accepted=False)
        ctx.equal("replayed old transfer cannot close newest stage",entry("access",fresh,a)["send"],True)
        ctx.equal("no access inherited from interim owner",entry("access",next_dm,a)["read"],False)
        ctx.equal("group with topic is distinct",entry("group","existing-space","--topic","x")["entry"] != group,True)
    return {"stages":[first,second,third],"late":native,"account_only":unclear,"dm_entries":[dm,next_dm,fresh]}
