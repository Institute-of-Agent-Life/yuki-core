"""Two independent stdio SDK peers exercise original scope and recover one confirmed effect."""

import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib, json, os, struct, subprocess, sys, zlib
from pathlib import Path
from tests.harness.compact import Owner, call, reply
from tests.harness.gateway_media import run as gateway_inbound
from tests.peers.chat.recorder import Recorder
from tests.peers.onebot.peer import Peer
from contextlib import closing


def png(pixel):
    def chunk(k, v):
        return (
            struct.pack(">I", len(v))
            + k
            + v
            + struct.pack(">I", zlib.crc32(k + v) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(b"\x00" + pixel))
        + chunk(b"IEND", b"")
    )


def installed(ctx, o, folders, sdk, model=None, visual=False, gateway=None):
    text = ""
    if model:
        text += f'\n[main]\nendpoint="{model.url}"\nmodel="fixture"\nname="Yuki"\nmax_output=512\nroot_slots=8\nmax_input_units=1200000\nmerge_ms=1\nvision_input={str(visual).lower()}\n'
    if model:
        text += (
            "\n[media]\ngateway_transfer_directory="
            + json.dumps(str(o.svc.data / "media-transfer"))
            + "\n"
        )
        text += f'\n[qq]\nnamespace="compact"\ndesignated_ingress="100"\n[[qq.bots]]\nself_id="100"\nstage="{o.bot["assignment"]}"\ntoken_env="YUKI_BROKER_QQ"\n'
    for plugin, folder in folders.items():
        text += (
            f'\n[[plugins]]\nid="{plugin}"\nnamespace="{plugin}"\nprogram='
            + json.dumps(sys.executable)
            + "\nargs="
            + json.dumps(
                [
                    str(ctx.root / "tests/peers/host-media/peer.py"),
                    str(folder),
                    str(sdk / "python-install"),
                ]
            )
            + '\ngrants=["media.open","media.publish","media.process","tool.call","forget.owner"]\n'
        )
        if gateway is not None and plugin == "codec-b":
            text = (
                text.rsplit("grants=", 1)[0]
                + 'grants=["media.open","media.publish","media.process","tool.call","forget.owner","gateway.inbound","gateway.send"]\n'
            )
            text += f'platform="telegram"\nbot_stage="{gateway}"\n'
        if plugin == "codec-a":
            text += '[[plugins.tools]]\nname="make_images"\ndescription="Paint two images"\ncacheable=false\nparallel_read=false\ncode_direct=false\nschema={type="object",properties={},additionalProperties=false}\n'
    with (o.svc.data / "yuki.toml").open("a") as f:
        f.write(text)


def ready(o, folders):
    for folder in folders.values():
        o.until(lambda: (folder / "ready.json").is_file())


def gateway_delivery(ctx, sdk, paper):
    owner = Owner(ctx, "gateway-version", 21)
    folders = {n: ctx.working / (n + "-gateway") for n in ["codec-a", "codec-b"]}
    for folder in folders.values():
        folder.mkdir()
    with owner.svc.running():
        bot = owner.op(
            "account", "open", "telegram", "codec-b", "bot-41", 1, "--start", 1
        )
        owner.op(
            "account",
            "open",
            "telegram",
            "codec-b",
            "room-42",
            1,
            "--person",
            owner.actor,
            "--start",
            1,
        )
    (folders["codec-a"] / "paper.dat").write_bytes(paper)
    for n, pixel in enumerate((b"\x01\x02\x03", b"\x21\x67\xc9")):
        (folders["codec-a"] / f"image-{n}.png").write_bytes(png(pixel))
    (folders["codec-a"] / "foreign.json").write_text(
        json.dumps({"artifact_id": "outside", "version": "0" * 64})
    )
    with Recorder() as model:
        installed(ctx, owner, folders, sdk, model, False, gateway=bot["assignment"])
        env = {**os.environ, "YUKI_BROKER_QQ": "synthetic-media-receiver"}
        with owner.svc.running(environment=env):
            ready(owner, folders)
            peer = folders["codec-b"]
            cmd = peer / "probe.tmp"
            cmd.write_text(
                json.dumps(
                    {
                        "method": "gateway.inbound",
                        "params": {
                            "source": "v4-witness",
                            "native_id": "99",
                            "sender": "room-42",
                            "occurred": 10,
                            "kind": "message",
                            "segments": [
                                {
                                    "type": "text",
                                    "text": "create a scoped media version",
                                }
                            ],
                            "accept": True,
                        },
                    }
                )
            )
            cmd.rename(peer / "probe.json")
            owner.until(lambda: (peer / "probed.json").exists())
            observed = json.loads((peer / "probed.json").read_bytes())
            ctx.equal(
                "authenticated gateway accepted original entry",
                observed["denied"],
                False,
            )
            event = observed["result"]["event"]
            _, q = owner.wire(model)
            reply(q, calls=[call("make", "make_images", {})])
            wire, q = owner.wire(model)
            created = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "make"
                )
            )["result"]["data"]["document"]
            args = {
                "artifact_id": created["artifact_id"],
                "attachment_kind": "file",
                "name": "paper.bin",
            }
            (peer / "tamper-plan").touch()
            reply(q, calls=[call("wrong-version", "send", args)])
            wire, q = owner.wire(model)
            refusal = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "wrong-version"
                )
            )
            ctx.check(
                "tampered gateway plan rejected before any physical part",
                refusal["state"] == "failed" and not (peer / "routed.json").exists(),
            )
            (peer / "tamper-plan").unlink()
            reply(q, calls=[call("right-version", "send", args)])
            wire, q = owner.wire(model)
            routed = json.loads((peer / "routed.json").read_bytes())
            settled = json.loads(
                next(
                    m["content"]
                    for m in wire["messages"]
                    if m.get("tool_call_id") == "right-version"
                )
            )
            ctx.equal(
                "original remote receiver reads bounded bytes for its own frozen route",
                (
                    settled["state"],
                    routed["digest"],
                    routed["version"],
                    routed["route"]["chat"],
                ),
                (
                    "confirmed",
                    hashlib.sha256(paper).hexdigest(),
                    created["version"],
                    "room-42",
                ),
            )
            reply(q, "done")
            owner.finished(event)
            ctx.equal(
                "only successful native child binds outbound artifact",
                owner.rows(
                    "SELECT count(*) FROM personal_artifact_references WHERE id=?",
                    created["artifact_id"],
                ),
                [(1,)],
            )
            return routed


def run(ctx):
    sdk = ctx.working / "sdk"
    dist = ctx.output / "sdk-distributions"
    p = subprocess.run(
        [
            sys.executable,
            str(ctx.root / "ci/publish-contract.py"),
            "build",
            "--out",
            str(dist),
            "--verify-work",
            str(sdk),
        ],
        capture_output=True,
        text=True,
    )
    if p.returncode:
        ctx.write("sdk-failure.json", {"stderr": p.stderr})
    ctx.equal("media schemas embedded in both actual SDK archives", p.returncode, 0)
    folders = {s: ctx.working / s for s in ["codec-a", "codec-b"]}
    for folder in folders.values():
        folder.mkdir()
    o = Owner(ctx, "media-broker", 21)
    installed(ctx, o, folders, sdk)
    raw = b"#!AMR\n" + b"original voice block." * 1350
    digest = hashlib.sha256(raw).hexdigest()
    (o.svc.data / "media-inputs" / "voice.amr").write_bytes(raw)

    def event(n, cache=True):
        frame = {
            "source": "original-" + str(n),
            "account": o.user["account"],
            "bot_stage": o.bot["assignment"],
            "kind": "message",
            "reply_bot": "100",
            "occurred": n,
            "observed": n,
            "segments": [
                {"type": "text", "text": "inspect actual bytes"},
                {
                    "type": "media",
                    "kind": "record",
                    "reference": "voice-native-" + str(n),
                },
            ],
        }
        eid = o.op("ingress", "record", json.dumps(frame), "--accept")["event"]
        ex = o.op("ingress", "claim", eid)["execution"]
        if cache:
            cached = o.op(
                "media",
                "cache-file",
                ex,
                o.actor,
                eid,
                0,
                o.bot["assignment"],
                "voice-native-" + str(n),
                "voice.amr",
            )
            ctx.equal("core loaded exact descriptor bytes", cached["digest"], digest)
        return eid, ex

    def process(ex, op, e):
        return o.op(
            "host",
            "process",
            "codec-a",
            ex,
            o.actor,
            op,
            json.dumps({"event": e, "slot": 0}),
            "transcript",
        )

    with o.svc.running(), ThreadPoolExecutor() as pool:
        ready(o, folders)
        eid, ex = event(100)
        f = folders["codec-a"]
        other = folders["codec-b"]
        (f / "hold-audio").touch()
        job = pool.submit(process, ex, "audio", eid)
        o.until(lambda: (f / "read-audio.json").is_file())
        capture = json.loads((f / "read-audio.json").read_bytes())
        handle = json.loads((f / "active-audio.json").read_bytes())["handle"]
        ctx.check(
            "two bounded reads carry only original bytes and version",
            capture["sha256"] == capture["declared"] == digest
            and capture["size"] == len(raw) > 16384,
        )
        probe = other / "probe.tmp"
        probe.write_text(
            json.dumps(
                {
                    "method": "media.open",
                    "params": {"handle": handle, "offset": 0, "length": 1},
                }
            )
        )
        probe.rename(other / "probe.json")
        o.until(lambda: (other / "probed.json").is_file())
        ctx.equal(
            "other real instance cannot read a live call handle",
            json.loads((other / "probed.json").read_bytes()),
            {"denied": True, "result": None},
        )
        (f / "hold-audio").unlink()
        ok = job.result()
        ctx.equal("original processor result confirmed", ok["state"], "confirmed")
        ctx.equal(
            "typed provenance records transcript without command promotion",
            o.rows(
                "SELECT source,segment,text FROM history_transcripts WHERE source=?",
                eid,
            ),
            [(eid, 1, "controlled transcription /ai admin")],
        )
        other_event, other_exec = event(102)
        (f / "hold-late").touch()
        late = pool.submit(process, other_exec, "late", other_event)
        o.until(lambda: (f / "read-late.json").is_file())
        automatic_holds = o.rows(
            "SELECT h.item,h.source FROM source_holds h JOIN host_calls c ON c.handle=h.item WHERE h.owner='codec-a' AND h.source=?", other_event
        )
        ctx.equal("selected media automatically registers its original holder", len(automatic_holds), 1)
        o.op("privacy", "erase", "erase-late-codec", o.actor, json.dumps([other_event]))
        (f / "hold-late").unlink()
        withheld = late.result()
        ctx.check(
            "erasure before returned ASR prevents late publication",
            not o.rows(
                "SELECT text FROM history_transcripts WHERE source=?", other_event
            )
            and withheld.get("result") is None,
        )
        o.until(lambda: o.rows("SELECT proof FROM owner_erasures WHERE owner='codec-a' AND proof IS NOT NULL"))
        automatic_forget = json.loads((f / "forget-erase-late-codec.json").read_bytes())
        ctx.equal("automatic media holder receives original forget source", automatic_forget["sources"], [other_event])
        ctx.equal("automatic media obligation is acknowledged", o.op("privacy", "pending", "codec-a")["obligations"], [])
        blocked_event, blocked_exec = event(104, False)
        outside = ctx.working / "outside-media"
        outside.mkdir()
        (outside / "voice.amr").write_bytes(b"unrelated outside source")
        escape = o.svc.data / "media-inputs" / "escape"
        if os.name == "nt":
            subprocess.run(
                ["cmd", "/d", "/c", "mklink", "/J", str(escape), str(outside)],
                capture_output=True,
                check=True,
            )
        else:
            escape.symlink_to(outside, target_is_directory=True)
        o.svc.command(
            "operator",
            "media",
            "cache-file",
            blocked_exec,
            o.actor,
            str(blocked_event),
            "0",
            o.bot["assignment"],
            "voice-native-104",
            "escape/voice.amr",
            accepted=False,
        )
        o.svc.command(
            "operator",
            "host",
            "process",
            "codec-a",
            blocked_exec,
            o.actor,
            "not-pinned",
            json.dumps({"event": blocked_event, "slot": 0}),
            "transcript",
            accepted=False,
        )
        ctx.check(
            "symlink source never turns into a delegated handle or reader payload",
            not (f / "active-not-pinned.json").exists()
            and not o.rows(
                "SELECT handle FROM host_calls WHERE operation='not-pinned'"
            ),
        )
        ctx.check(
            "unselected second processor received no media-process request",
            not list(other.glob("active-*.json")),
        )
        ctx.equal(
            "no model purchase for local selected processing",
            o.rows("SELECT count(*) FROM model_attempts"),
            [(0,)],
        )
    for folder in folders.values():
        (folder / "ready.json").unlink(missing_ok=True)
    with o.svc.running():
        ready(o, folders)
        original = process(ex, "audio", eid)
        ctx.check(
            "cold original operation returns a retained receipt",
            original["state"] == "confirmed"
            and o.rows("SELECT count(*) FROM host_calls WHERE operation='audio'")
            == [(1,)],
        )
    visuals = []
    for vision in (False, True):
        actor = Owner(ctx, "presentation-" + str(vision), 21)
        vf = {n: ctx.working / (n + str(vision)) for n in ["codec-a", "codec-b"]}
        for folder in vf.values():
            folder.mkdir()
        pics = [png(b"\xf1\x45\x21"), png(b"\x04\xc1\x86")]
        for n, pic in enumerate(pics):
            (vf["codec-a"] / f"image-{n}.png").write_bytes(pic)
        (vf["codec-a"] / "foreign.json").write_text(
            json.dumps({"artifact_id": "foreign-plugin-object", "version": "0" * 64})
        )
        paper = b"pinned plugin bytes " * 1200
        (vf["codec-a"] / "paper.dat").write_bytes(paper)
        env = {**os.environ, "YUKI_BROKER_QQ": "synthetic-media-receiver"}
        with Recorder() as model:
            installed(ctx, actor, vf, sdk, model, vision)
            with actor.svc.running(environment=env), closing(
                Peer(actor.svc.address, "100", env["YUKI_BROKER_QQ"])
            ) as qq:
                ready(actor, vf)
                event_id = actor.human("show two selected images")
                first, q = actor.wire(model)
                names = [t["function"]["name"] for t in first["tools"]]
                ctx.check(
                    "normal paid Main declares actual installed publishing tool",
                    "make_images" in names,
                )
                reply(q, calls=[call("paint", "make_images", {})])
                wire, q = actor.wire(model)
                receipt = json.loads(
                    next(
                        m["content"]
                        for m in wire["messages"]
                        if m.get("tool_call_id") == "paint"
                    )
                )
                rows = actor.rows(
                    "SELECT status FROM host_media_presentations ORDER BY ordinal"
                )
                ctx.equal(
                    "two local images and foreign handle have independent presentation outcomes",
                    rows,
                    [(("ready" if vision else "no_vision"),)] * 2 + [("unavailable",)],
                )
                ctx.equal(
                    "handler effect remains confirmed", receipt["state"], "confirmed"
                )
                images = [
                    part["image_url"]["url"]
                    for m in wire["messages"]
                    if isinstance(m.get("content"), list)
                    for part in m["content"]
                    if part.get("type") == "image_url"
                ]
                ctx.equal(
                    "native images require the selected original profile",
                    images,
                    (
                        [
                            "data:image/png;base64," + base64.b64encode(p).decode()
                            for p in pics
                        ]
                        if vision
                        else []
                    ),
                )
                published = receipt["result"]["data"]["document"]
                ctx.check(
                    "published document digest comes from the open original file",
                    published["version"] == hashlib.sha256(paper).hexdigest()
                    and (vf["codec-a"] / "paper.dat").read_bytes() != paper,
                )
                reply(
                    q,
                    calls=[
                        call(
                            "deliver-version",
                            "send",
                            {
                                "artifact_id": published["artifact_id"],
                                "attachment_kind": "file",
                                "name": "paper.bin",
                            },
                        )
                    ],
                )
                upload = qq.receive()
                ctx.equal(
                    "original QQ destination receives selected immutable bytes",
                    (
                        upload["action"],
                        upload["params"]["user_id"],
                        Path(upload["params"]["file"]).read_bytes(),
                    ),
                    ("upload_private_file", 200, paper),
                )
                qq.send(
                    {"echo": upload["echo"], "status": "ok", "retcode": 0, "data": {}}
                )
                final, q = actor.wire(model)
                original = json.loads(
                    next(
                        m["content"]
                        for m in final["messages"]
                        if m.get("tool_call_id") == "deliver-version"
                    )
                )
                ctx.equal(
                    "presentation and actual Social part keep separate receipts",
                    original["state"],
                    "confirmed",
                )
                reply(q, "done")
                actor.finished(event_id)
                ctx.equal(
                    "only three original Main wires with the selected delivery",
                    actor.rows("SELECT count(*) FROM model_attempts"),
                    [(3,)],
                )
            for folder in vf.values():
                (folder / "ready.json").unlink(missing_ok=True)
            with actor.svc.running(environment=env):
                ready(actor, vf)
                ctx.equal(
                    "cold owner retains exactly one external effect",
                    len((vf["codec-a"] / "executions.log").read_text().splitlines()),
                    1,
                )
                ctx.equal(
                    "committed effect stays readable from its original record",
                    actor.rows("SELECT state FROM host_calls WHERE method='tool.call'"),
                    [("confirmed",)],
                )
            visuals.append(
                {
                    "vision": vision,
                    "images": len(images),
                    "presentation": rows,
                    "handler_runs": 1,
                }
            )
    return {
        "binary_sha256": hashlib.sha256(
            (
                ctx.root
                / "target"
                / "debug"
                / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
            ).read_bytes()
        ).hexdigest(),
        "media_schema_sha256": hashlib.sha256(
            (ctx.root / "protocol/plugin/media/methods.json").read_bytes()
        ).hexdigest(),
        "negotiated_media_methods": sorted(
            m
            for m in json.loads((folders["codec-a"] / "ready.json").read_bytes())[
                "methods"
            ]
            if m.startswith("media.")
        ),
        "gateway_delivery": gateway_delivery(ctx, sdk, paper),
        "gateway_inbound": gateway_inbound(ctx, sdk),
        "original_digest": digest,
        "physical_reader": capture,
        "foreign_handle_denied": True,
        "late_result": withheld,
        "automatic_source_holds": automatic_holds,
        "automatic_forget": automatic_forget,
        "cases": {"R13": visuals},
        "host_idl_sha256": hashlib.sha256(
            (ctx.root / "sdk/rust/src/host-preview.json").read_bytes()
        ).hexdigest(),
    }
