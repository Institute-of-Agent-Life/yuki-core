"""Exercise the DQ decision without treating artificial pairs as a deployment witness."""
import copy
import importlib.util
import json
import hashlib

def run(ctx):
    file = ctx.root / "baseline/qq-key/decision.py"
    spec = importlib.util.spec_from_file_location("qq_decision", file)
    decision = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(decision)
    fixtures = ctx.root / "fixtures/qq-dual"
    context = json.loads((fixtures / "rehearsal.json").read_bytes())
    batches = context["observations"]
    valid = decision.assess(context)
    ctx.equal("labeled two-bot pairs support only a rehearsal", valid["group"]["mode"], "scoped_shared_key")
    ctx.equal("rehearsal never authorizes target use", valid["deployment_verified"], False)
    ctx.equal("same-second same-text pair remains two keys", len(set(tuple(x) for x in valid["group"]["pair_keys"].values())), 2)
    ctx.check("receiver locators remain distinct", all(len(set(tuple(i) for i in items)) >= 2 for items in valid["group"]["locators"].values()))
    ctx.equal("group evidence grants no private shared key", valid["private"]["mode"], "per_receiving_bot")
    solo = copy.deepcopy(context)
    solo["observations"] = [r for r in batches if r["packet"]["self_id"] == "bot-A"]
    ctx.equal("one receiver cannot establish a shared key", decision.assess(solo)["group"]["mode"], "designated_ingress")
    bad = copy.deepcopy(context)
    bad["observations"][0]["packet"]["message_seq"] = 999
    ctx.equal("mismatched group sequence rejects sharing", decision.assess(bad)["group"]["mode"], "designated_ingress")
    bad = copy.deepcopy(context)
    for row in bad["observations"]:
        if row.get("occurrence") == "second": row["packet"]["message_seq"] = 101
    ctx.equal("distinct events cannot collide", decision.assess(bad)["group"]["mode"], "designated_ingress")
    collision = copy.deepcopy(context)
    for r in collision["observations"]:
        if r.get("occurrence") == "second" and r["packet"]["self_id"] == "bot-A":
            r["packet"]["message_id"] = 11
    ctx.equal("one local locator cannot identify both originals", decision.assess(collision)["group"]["mode"], "designated_ingress")
    corrupt = copy.deepcopy(context)
    corrupt["kind"] = "target_capture"
    corrupt["observations"] = [{"file":"deidentified.json","sha256":"0"*64}]
    capture = ctx.working / "capture.json"
    capture.write_text(json.dumps(corrupt))
    (ctx.working / "deidentified.json").write_text("{}")
    ctx.rejects("target packets are checked against capture digest", ValueError, lambda: decision.load(capture))
    shuffled = copy.deepcopy(context)
    shuffled["observations"].reverse()
    ctx.equal("arrival order cannot select route or key", decision.assess(shuffled)["group"], valid["group"])
    missing = copy.deepcopy(context)
    missing["target_image_digest"] = "sha256:" + "2" * 64
    ctx.equal("changed image requires fresh capture", decision.assess(missing)["group"]["mode"], "designated_ingress")
    deployed = decision.assess(json.loads((fixtures / "deployment.json").read_bytes()))
    ctx.equal("current QQ evidence admits designated ingress only", deployed["group"]["mode"], "designated_ingress")
    ctx.check("disconnect gap explicit", deployed["group"]["offline_loss_possible"])
    return {"deployment":deployed,"synthetic_rehearsal":valid,
            "rehearsal_sha256":hashlib.sha256((fixtures/"rehearsal.json").read_bytes()).hexdigest()}
