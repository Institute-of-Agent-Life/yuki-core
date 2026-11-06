"""Replay the Main media handoff without modifying the external project or contacting a provider."""

from pathlib import Path
import argparse, base64, hashlib, json, os, shutil, sys, uuid, zipfile

H = lambda b: hashlib.sha256(b).hexdigest()
parser = argparse.ArgumentParser(
    description="Exercise an independently built EXT-031 package with a verified Windows core preview"
)
for arg in ("source", "core-zip", "consumer-evidence", "package-index", "scratch-root"):
    parser.add_argument("--" + arg, type=Path, required=True)
parser.add_argument("--core-sha256", required=True)
args = parser.parse_args()
if os.name != "nt":
    parser.error("this witness uses the pinned Windows decoder")
base = args.scratch_root.resolve()
base.mkdir(parents=True, exist_ok=True)
source = args.source.resolve()
old = args.consumer_evidence.resolve()
frozen = json.loads((source / ".yuki-source.json").read_bytes())
assert not frozen["dirty"]
w = base / ("CORE-038-real-ext031-" + uuid.uuid4().hex[:10])
w.mkdir(exist_ok=False)
out = w / "proof"
out.mkdir()
sys.dont_write_bytecode = True
sys.path.insert(0, str(source))
from tests.harness import Context
from tests.harness.compact import Owner, call, reply
from tests.peers.chat.recorder import Recorder

ctx = Context(w, out, {"seed": 0, "clock": 0})
ctx.working = w / "cases"
ctx.working.mkdir()
for n, expected in frozen["files"].items():
    if (
        n.startswith("tests/harness/")
        or n.startswith("tests/peers/chat/")
        or n.startswith("migrations/")
    ):
        raw = (source / n).read_bytes()
        assert H(raw) == expected
        if n.startswith("migrations/"):
            (w / n).parent.mkdir(exist_ok=True)
            (w / n).write_bytes(raw)
arc = args.core_zip.resolve()
assert H(arc.read_bytes()) == args.core_sha256
with zipfile.ZipFile(arc) as z:
    c = json.loads(z.read("candidate.json"))
    assert (c["source_commit"], c["source_tree_sha256"]) == (
        frozen["commit"],
        frozen["tree_sha256"],
    )
    exe = z.read("bin/yuki-core.exe")
    assert H(exe) == c["members"]["bin/yuki-core.exe"]["sha256"]
    dest = w / "target/debug/yuki-core.exe"
    dest.parent.mkdir(parents=True)
    dest.write_bytes(exe)
prev = json.loads((old / "media-inspection.json").read_bytes())
pkg = w / "ext-package"
pkg.mkdir()
ext_package_proof = json.loads(args.package_index.read_bytes())["artifacts"]
for name, expect in ext_package_proof.items():
    raw = (old / "packages" / name).read_bytes()
    assert H(raw) == expect, name
    p = pkg / name
    p.parent.mkdir(exist_ok=True, parents=True)
    p.write_bytes(raw)
assert H((pkg / "yuki-media.exe").read_bytes()) == prev["plugin_sha256"]
codecs = old / "work/codecs"
runtime = w / "ext-runtime"
runtime.mkdir()
for name in ["Scripts", "Lib"]:
    shutil.copytree(codecs / name, runtime / name)
shutil.copyfile(codecs / "pyvenv.cfg", runtime / "pyvenv.cfg")
ff = runtime / "ffmpeg.exe"
shutil.copyfile(codecs / "ffmpeg.exe", ff)
expected = json.loads((pkg / "packaging/codecs.lock").read_bytes())["ffmpeg"][
    "platforms"
]["windows-x86_64"]["binary_sha256"]
assert H(ff.read_bytes()) == expected
temp = runtime / "tmp"
temp.mkdir()
env = dict(
    os.environ,
    YUKI_MEDIA_PYTHON=str(runtime / "Scripts/python.exe"),
    YUKI_MEDIA_FFMPEG=str(ff),
    YUKI_MEDIA_TEMP=str(temp),
    YUKI_BROKER_QQ="synthetic-media-source",
)
evidence = {
    "core": c["source_commit"],
    "tree": c["source_tree_sha256"],
    "sdk_versions": c["sdk_versions"],
    "core_archive_sha256": H(arc.read_bytes()),
    "consumer_unit": "EXT-031",
    "consumer_snapshot_sha256": json.loads((old / "manifest.json").read_bytes())[
        "source_snapshot_sha256"
    ],
    "consumer_binary_sha256": prev["plugin_sha256"],
    "ffmpeg_sha256": expected,
    "cases": {},
    "published": False,
    "live_gateway": False,
    "paid_requests": 0,
}
for name, kind, filename, visual in [
    ("pdf", "file", "good.pdf", False),
    ("video", "video", "video.mp4", True),
]:
    o = Owner(ctx, name, 49)
    data = (codecs / filename).read_bytes()
    digest = H(data)
    assert digest == prev[name]["result"]["data"]["version"]
    (o.svc.data / "media-inputs" / filename).write_bytes(data)
    with Recorder() as model:
        cfg = (
            "\n[main]\nendpoint="
            + json.dumps(model.url)
            + '\nmodel="fixture"\nname="Yuki"\nmax_output=512\nroot_slots=8\nmax_input_units=1200000\nmerge_ms=1\nvision_input='
            + ("true" if visual else "false")
            + '\nattachment_processor="media"\n'
        )
        cfg += (
            "\n[media]\ngateway_transfer_directory="
            + json.dumps(str(o.svc.data / "transfer"))
            + '\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage='
            + json.dumps(o.bot["assignment"])
            + '\ntoken_env="YUKI_BROKER_QQ"\n'
        )
        cfg += (
            '\n[[plugins]]\nid="media"\nnamespace="media"\nprogram='
            + json.dumps(str(pkg / "yuki-media.exe"))
            + '\ngrants=["media.process","media.open","media.publish","forget.owner"]\nenv=["YUKI_MEDIA_PYTHON","YUKI_MEDIA_FFMPEG","YUKI_MEDIA_TEMP"]\n'
        )
        (o.svc.data / "transfer").mkdir(exist_ok=True)
        with (o.svc.data / "yuki.toml").open("a", encoding="utf-8") as file:
            file.write(cfg)
        with o.svc.running(environment=env):
            o.until(
                lambda: (
                    o.rows("SELECT online FROM host_installations WHERE plugin='media'")
                    == [(1,)]
                )
            )
            eid = o.op(
                "ingress",
                "record",
                json.dumps(
                    {
                        "source": name,
                        "account": o.user["account"],
                        "bot_stage": o.bot["assignment"],
                        "reply_bot": "100",
                        "observed": 1,
                        "occurred": 1,
                        "kind": "message",
                        "segments": [
                            {
                                "type": "media",
                                "kind": "file",
                                "reference": "unselected",
                            },
                            {
                                "type": "media",
                                "kind": kind,
                                "reference": "original-file",
                            },
                        ],
                    }
                ),
                "--accept",
            )["event"]
            initial, q = o.wire(model)
            ex = o.rows("SELECT id FROM executions WHERE source=?", eid)[0][0]
            assert (
                o.op(
                    "media",
                    "cache-file",
                    ex,
                    o.actor,
                    eid,
                    1,
                    o.bot["assignment"],
                    "original-file",
                    filename,
                )["digest"]
                == digest
            )
            reply(
                q,
                calls=[
                    call(
                        "select",
                        "inspect_conversation_attachment",
                        {
                            "event_id": eid,
                            "attachment_index": 1,
                            "question": "inspect these original bytes",
                        },
                    )
                ],
            )
            body, q = o.wire(model, timeout=45)
            receipt = next(
                m["content"]
                for m in body["messages"]
                if m.get("tool_call_id") == "select"
            )
            proof = json.loads(receipt)
            r = proof["result"]["data"]
            assert (
                proof["state"] == "confirmed"
                and r["version"] == digest
                and r["source"] == {"event": eid, "slot": 1}
            )
            actual = [
                H(
                    base64.b64decode(
                        b["image_url"]["url"].split(",", 1)[1], validate=True
                    )
                )
                for m in body["messages"]
                if isinstance(m.get("content"), list)
                for b in m["content"]
                if b.get("type") == "image_url"
            ]
            if name == "pdf":
                assert (
                    r["text"] == prev["pdf"]["result"]["data"]["text"] and actual == []
                )
            else:
                assert (
                    actual
                    == [f["sha256"] for f in prev["video"]["result"]["data"]["frames"]]
                    == [f["sha256"] for f in r["frames"]]
                )
                assert [f["seconds"] for f in r["frames"]] == [0.0, 5.0] and [
                    f["state"] for f in r["images"]
                ] == ["ready", "ready"]
            reply(q, "")
            o.finished(eid)
            attempts = o.rows("SELECT count(*) FROM model_attempts")[0][0]
            assert attempts == 2 and o.rows(
                "SELECT count(*) FROM host_calls WHERE method='media.process'"
            ) == [(1,)]
            evidence["cases"][name] = {
                "input_sha256": digest,
                "event": eid,
                "slot": 1,
                "execution": ex,
                "model_attempts": attempts,
                "physical_processor_calls": 1,
                "http_frame_sha256": actual,
                "receipt": r,
                "http_request_sha256": H(json.dumps(body, sort_keys=True).encode()),
            }
            (out / (name + "-request.json")).write_text(
                json.dumps(body, ensure_ascii=False), encoding="utf-8"
            )
(out / "real-extension.json").write_text(
    json.dumps(evidence, indent=2), encoding="utf-8"
)
print(
    json.dumps(
        {
            "evidence": str(out),
            "receipt_sha256": H((out / "real-extension.json").read_bytes()),
        }
    )
)
