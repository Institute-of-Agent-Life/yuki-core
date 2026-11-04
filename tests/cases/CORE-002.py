"""Compare two observations from v4, reject damaged copies, then index the evidence."""
import copy
import json

from baseline.replay.compare import validate_bundle, compare
from baseline.replay.package import make_candidate


def run(ctx):
    bundle = validate_bundle(ctx.root)
    a, b = bundle["runs"]
    ctx.equal("pinned v4 replays agree", compare(a, b), [])
    broken = copy.deepcopy(b)
    lost = broken["next_main"]["gateway_calls"].pop()
    ctx.equal("removed a confirmed outward send", lost[0], "send_group_msg")
    ctx.check("a lost delivery fails the comparison", bool(compare(a, broken)))
    damaged = copy.deepcopy(b)
    frames = damaged["signature"]["wire_requests"]
    original = json.dumps(frames)
    damaged["signature"]["wire_requests"] = json.loads(original.replace("original-native-signature-", "changed-signature-"))
    ctx.check("a changed native signature fails the comparison", bool(compare(a, damaged)))
    for label in ("next_main", "signature", "app_work", "partial", "wire4"):
        ctx.check("actual v4 evidence: " + label, bool(a[label]))
    ctx.equal("original connector handshake", a["wire4"]["frames"][0]["body"]["protocol_version"], 4)
    ctx.equal("recorded workload sizes", bundle["workload"]["counts"],
              {"private": 200, "group:20001": 500, "group:20002": 500, "long_works": 20})
    candidate = make_candidate(ctx.root, ctx.output, bundle["index"])
    return {**bundle["index"], "candidate": candidate}
