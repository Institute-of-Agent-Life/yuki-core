"""Exercise the native operator, fixed Admin wire, physical HTTP and interrupted file saves."""
import copy
import hashlib
import json
import os
import subprocess
import sys
import uuid
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from tests.harness.compact import Owner, call, reply
from tests.harness.crash import kill_writer
from tests.harness.personal import matches_reply
from tests.peers.chat.recorder import Recorder as BaseRecorder


class Recorder(BaseRecorder):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.authorization_hashes = []
        record = self.authorization_hashes
        original = self.server.RequestHandlerClass.do_POST
        def post(handler):
            record.append(hashlib.sha256(handler.headers.get("Authorization", "").encode()).hexdigest())
            return original(handler)
        self.server.RequestHandlerClass.do_POST = post


def credential_hash(value):
    return hashlib.sha256(("Bearer " + value).encode()).hexdigest()


def request_id():
    return str(uuid.uuid4())


def wire(address, path, token=None, body=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    request = Request("http://" + address + path, headers=headers,
                      data=None if body is None else json.dumps(body).encode())
    try:
        with urlopen(request, timeout=12) as response:
            return response.status, json.load(response)
    except HTTPError as error:
        with error:
            return error.code, json.load(error)


def setup(ctx, name, recorder, *, pending=False):
    owner = Owner(ctx, name, 20)
    if pending:
        with owner.svc.running():
            owner.pending_event = owner.human("Original response must survive a profile change and cold restart")
    owner.config(recorder)
    settings = owner.svc.data / "yuki.toml"
    settings.write_text('model_profiles_file="model_profiles.toml"\n'
                        'operator_registry_file="operators.toml"\n' + settings.read_text())
    root = recorder.url.removesuffix("/chat/completions")
    profile = f'''schema_version = 3
deployment_note = 'untouched deployment value'
# original deployment annotation
[profiles.primary]
provider = "fixture"
protocol = "chat_completions"
base_url = "{root}"
api_key_env = "PROFILE_FIXTURE_KEY"
model = "before" # preserve this comment
timeout_seconds = 10.0
max_retries = 2
default_temperature = 0.4
default_max_output_tokens = 128
thinking_mode = "disabled"
capabilities = ["tools"]
deployment_private_option = "preserve-server-option"
headers = {{ X-Profile-Canary = "private-header-canary" }}
[routes]
chat_agent = "primary"
memory_extraction = "primary"
memory_self_reflection = "primary"
memory_dream = "primary"
plugin_agent_session = "primary"
utility_structured = "primary"
conversation_compaction = "retired-profile"
'''
    (owner.svc.data / "model_profiles.toml").write_text(profile)
    secret = owner.svc.data / "model_profiles.secrets.json"
    secret.write_text(json.dumps({"version": 1, "keys": {"PROFILE_FIXTURE_KEY": "fixture-key-before"}}))
    secret.chmod(0o600)
    declarations = []
    tokens = {}
    for name, grants in [("writer_a", True), ("writer_b", True), ("unprivileged", False)]:
        alias = "PROFILE_OPERATOR_" + name.upper()
        tokens[alias] = "fixture-operator-token-" + name + "-0000000000000000"
        caps = ["control.config.file.content.read", "control.config.file.mutate", "control.operation.read"] if grants else []
        declarations.append(f'[[operators]]\nprincipal_id="{request_id()}"\ntoken_env="{alias}"\n'
                            f'person_id="{owner.actor}"\ncapabilities={json.dumps(caps)}\n')
    (owner.svc.data / "operators.toml").write_text("\n".join(declarations))
    return owner, {**os.environ, **tokens}


def body(view, model):
    document = copy.deepcopy(view["document"])
    document["profiles"]["primary"]["model"] = model
    return {"request_id": request_id(), "expected_revision": view["revision"], "document": document}


def file_hashes(owner):
    return {name: hashlib.sha256((owner.svc.data / name).read_bytes()).hexdigest()
            for name in ("model_profiles.toml", "model_profiles.secrets.json")}


def card_module(ctx, card):
    import importlib.util
    spec = importlib.util.spec_from_file_location(card, ctx.root / "tests/cases" / (card + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def auxiliary_routes(ctx, sdk):
    aux = card_module(ctx, "CORE-050")
    results = {}
    for task in ("memory_self_reflection", "memory_dream", "plugin_agent_session", "utility_structured"):
        with Recorder() as old, Recorder() as new:
            owner, env = setup(ctx, "profile-task-" + task, old)
            with owner.svc.running(environment=env):
                entry = owner.op("entry", "private", owner.user["assignment"], owner.bot["assignment"])["entry"]
            folder = ctx.working / ("profile-peer-" + task)
            aux.install(ctx, owner, folder, sdk, old, entry)
            settings = owner.svc.data / "yuki.toml"
            settings.write_text(settings.read_text().replace("[plugins.aux_routes.jev.profile]", "[plugins.aux_routes.jev.profile]\nprofile_task=" + json.dumps(task)))
            with owner.svc.running(environment=env):
                owner.until((folder / "ready.json").exists)
                source = aux.event(owner, "task-before")
                owner.until((folder / ("event-" + str(source) + ".json")).exists)
                peer = aux.Peer(owner, folder)
                params = {"origin": {"event": source}, "call_id": "original", "route": "jev",
                          "purpose": "review", "text": "Follow the installed task route"}
                first = peer.submit("llm.complete", params)
                raw, answer = old.receive()
                ctx.equal(task + " selects catalog before physical request", json.loads(raw)["model"], "before")
                view = owner.op("profiles", "read")
                update = body(view, "before")
                target = copy.deepcopy(update["document"]["profiles"]["primary"])
                target.update(model=task + "-after", base_url=new.url.removesuffix("/chat/completions"))
                update["document"]["profiles"]["next"] = target
                update["document"]["routes"][task] = "next"
                update["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-after"}
                receipt = owner.op("profiles", "apply", json.dumps(update))
                ctx.equal(task + " route activates atomically", receipt["effective_state"]["status"], "applied")
                reply(answer, "original auxiliary response")
                paid = peer.receive(first)
                ctx.equal(task + " original response retains its request", paid["state"], "response")
                replay = peer.call("llm.complete", params)
                ctx.equal(task + " replay retains original logical", replay["logical"], paid["logical"])
                second = peer.submit("llm.complete", {**params, "call_id": "next"})
                raw, answer = new.receive()
                ctx.equal(task + " next logical request uses new route", json.loads(raw)["model"], task + "-after")
                reply(answer, "next auxiliary response")
                fresh = peer.receive(second)
                ctx.check(task + " next call is separately accounted", fresh["logical"] != paid["logical"])
                ctx.equal(task + " old and new credentials stay with original calls",
                          [old.authorization_hashes[0], new.authorization_hashes[0]],
                          [credential_hash("fixture-key-before"), credential_hash("fixture-key-after")])
                ctx.check(task + " replay issues no second physical request", old.wires.empty() and new.wires.empty())
                ctx.equal(task + " retains installed root budget", owner.rows("SELECT max_requests,reserved_requests FROM root_budgets"), [(2, 2)])
                results[task] = {"original": paid, "next": fresh, "receipt": receipt}
    return results


def extraction_route(ctx):
    aux = card_module(ctx, "CORE-050")
    with Recorder() as old, Recorder() as new:
        owner, env = setup(ctx, "profile-extraction-route", old)
        with (owner.svc.data / "yuki.toml").open("a") as f:
            f.write('\n[extraction]\nmode="json_schema"\ntrigger_count=1\n'
                    '[extraction.profile]\nendpoint=' + json.dumps(old.url) +
                    '\nmodel="unused-flat-profile"\nname="Yuki"\nmerge_ms=0\nroot_slots=1\nmax_input_units=300000\nmax_output=4096\n')
        with owner.svc.running(environment=env):
            source = aux.event(owner, "extract-profile-first", "Original extraction fact")
            raw, answer = old.receive()
            ctx.equal("extraction selects task route", json.loads(raw)["model"], "before")
            update = body(owner.op("profiles", "read"), "before")
            target = copy.deepcopy(update["document"]["profiles"]["primary"])
            target.update(model="extract-next", base_url=new.url.removesuffix("/chat/completions"), structured_output_mode="json_schema")
            update["document"]["profiles"]["extract"] = target
            update["document"]["routes"]["memory_extraction"] = "extract"
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-after"}
            receipt = owner.op("profiles", "apply", json.dumps(update))
            reply(answer, json.dumps({"claims": []}))
            owner.until(lambda: owner.rows("SELECT count(*) FROM extraction_batches WHERE state='committed'") == [(1,)])
            aux.event(owner, "extract-profile-next", "Next extraction fact")
            raw, answer = new.receive()
            ctx.equal("next extraction batch selects new route", json.loads(raw)["model"], "extract-next")
            ctx.equal("structured mode comes from selected profile", json.loads(raw)["response_format"]["type"], "json_schema")
            reply(answer, json.dumps({"claims": []}))
            owner.until(lambda: owner.rows("SELECT count(*) FROM extraction_batches WHERE state='committed'") == [(2,)])
            ctx.equal("extraction preserves original paid profiles", owner.rows("SELECT json_extract(task,'$.profile.model') FROM extraction_batches ORDER BY rowid"), [("before",), ("extract-next",)])
            ctx.equal("extraction credentials follow each original batch", [old.authorization_hashes[0], new.authorization_hashes[0]], [credential_hash("fixture-key-before"), credential_hash("fixture-key-after")])
            ctx.check("task-only route change does not replace chat route", owner.rows("SELECT json_extract(value,'$.model') FROM core_meta WHERE key='model-profile-current'") == [("before",)])
            return {"receipt": receipt, "original_source": source, "batches": 2}


def embedding_restart(ctx):
    vectors = card_module(ctx, "CORE-028")
    with Recorder() as chat, Recorder("/v1/embeddings") as old, Recorder("/v1/embeddings") as new:
        owner, env = setup(ctx, "profile-embedding-restart", chat)
        settings = owner.svc.data / "yuki.toml"
        text = settings.read_text().replace('mode="lexical-only"', 'mode="hybrid"')
        text += '\n[memory.embedding]\nprovider="openai_compatible"\nmodel="unused-deployment-default"\nbase_url=' + json.dumps(old.url.removesuffix("/embeddings")) + '\ndimensions=2\n'
        settings.write_text(text)
        catalog = owner.svc.data / "model_profiles.toml"
        catalog.write_text(catalog.read_text() + '\n[embedding]\nenabled=true\nprovider="openai_compatible"\nmodel="embedding-before"\nbase_url=' + json.dumps(old.url.removesuffix("/embeddings")) + '\ndimensions=2\napi_key_env="PROFILE_FIXTURE_KEY"\n')
        with owner.svc.running(environment=env):
            event = owner.human("Original tea fact")
            _, chat_answer = owner.wire(chat)
            execution = owner.rows("SELECT id FROM executions WHERE source=?", event)[0][0]
            edit = {"action": "add", "subject": {"kind": "person", "person": owner.actor},
                    "fact_key": "profile-tea", "body": "Original tea fact",
                    "evidence": [{"kind": "event", "id": event, "quote": "Original tea fact"}]}
            fact = owner.op("memory", "edit", execution, owner.actor, "profile-tea", json.dumps(edit))
            raw, answer = old.receive()
            ctx.equal("boot embedding reads model catalog", json.loads(raw)["model"], "embedding-before")
            first_profile = owner.op("memory", "vector-status")["profile"]
            update = body(owner.op("profiles", "read"), "before")
            update["document"]["embedding"].update(model="embedding-after", base_url=new.url.removesuffix("/embeddings"))
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-after"}
            receipt = owner.op("profiles", "apply", json.dumps(update))
            ctx.equal("embedding change reports restart", receipt["effective_state"]["status"], "saved_pending_restart")
            ctx.equal("embedding save preserves loaded connection", owner.op("memory", "vector-status")["profile"], first_profile)
            vectors.vector_result(answer, [(0, [1, 0])])
            owner.until(lambda: owner.op("memory", "vector-status")["covered"] == 1)
            ctx.check("non-chat save does not dispatch replacement before restart", new.wires.empty())
            reply(chat_answer, "Fact retained")
            owner.finished(event)
        with owner.svc.running(environment=env):
            raw, answer = new.receive()
            ctx.equal("cold restart applies saved non-chat connection", json.loads(raw)["model"], "embedding-after")
            vectors.vector_result(answer, [(0, [0, 1])])
            owner.until(lambda: owner.op("memory", "vector-status")["covered"] == 1)
            next_profile = owner.op("memory", "vector-status")["profile"]
            ctx.check("new model and credential use distinct vector identity", next_profile != first_profile)
            ctx.equal("sidecar embedding credentials reach only their physical requests", [old.authorization_hashes[0], new.authorization_hashes[0]], [credential_hash("fixture-key-before"), credential_hash("fixture-key-after")])
            ctx.equal("booted revision is reported loaded", owner.op("profiles", "read")["matches_loaded"], True)
            return {"receipt": receipt, "fact": fact["fact"], "before": first_profile, "after": next_profile}


def search_profile(ctx):
    search = card_module(ctx, "CORE-024")
    with Recorder() as chat, Recorder("/v1/responses") as old, Recorder("/v1/responses") as new:
        owner, env = setup(ctx, "profile-independent-search", chat)
        env["YUKI_FIXTURE_SEARCH"] = "synthetic-search-only"
        settings = owner.svc.data / "yuki.toml"
        with settings.open("a") as f: f.write(search.cfg(old, "openai_responses"))
        catalog = owner.svc.data / "model_profiles.toml"
        text = catalog.read_text().replace('schema_version = 3', 'schema_version = 3\nsearch_connection = "lookup"')
        text += '\n[profiles.lookup]\nprovider="openai"\nprotocol="responses"\nmodel="search-before"\nbase_url=' + json.dumps(old.url.removesuffix("/responses")) + '\napi_key_env="PROFILE_FIXTURE_KEY"\ntimeout_seconds=10.0\nmax_retries=0\ndefault_temperature=0.4\ndefault_max_output_tokens=128\nthinking_mode="disabled"\ncapabilities=["native_web_search"]\n'
        catalog.write_text(text)
        with owner.svc.running(environment=env):
            first = owner.human("Keep this private Main chain separate from search")
            _, answer = owner.wire(chat)
            update = body(owner.op("profiles", "read"), "chat-after-search-change")
            update["document"]["profiles"]["lookup"].update(model="search-after", base_url=new.url.removesuffix("/responses"))
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-after"}
            receipt = owner.op("profiles", "apply", json.dumps(update))
            reply(answer, calls=[call("old-search", "web_search", {"query": "original tea"})])
            raw, response = old.receive()
            ctx.equal("old Main uses its original independent search profile", json.loads(raw)["model"], "search-before")
            ctx.check("independent search excludes private Main context", "private Main chain" not in raw)
            response.put((search.source("openai_responses"), 0))
            _, answer = owner.wire(chat)
            reply(answer, "Original search settled")
            owner.finished(first)
            fresh = owner.human("Use the new independent search profile")
            _, answer = owner.wire(chat)
            reply(answer, calls=[call("new-search", "web_search", {"query": "new tea"})])
            raw, response = new.receive()
            ctx.equal("new Main uses saved independent search profile", json.loads(raw)["model"], "search-after")
            response.put((search.source("openai_responses"), 0))
            _, answer = owner.wire(chat)
            reply(answer, "New search settled")
            owner.finished(fresh)
            ctx.equal("search credentials remain attached to each Main snapshot", [old.authorization_hashes[0], new.authorization_hashes[0]], [credential_hash("fixture-key-before"), credential_hash("fixture-key-after")])
            disabled = body(owner.op("profiles", "read"), "chat-after-search-change")
            disabled["document"]["search"] = {"mode": "disabled"}
            disabled_receipt = owner.op("profiles", "apply", json.dumps(disabled))
            ctx.equal("global search mode reports required restart", disabled_receipt["effective_state"]["status"], "saved_pending_restart")
        with owner.svc.running(environment=env):
            execution = owner.rows("SELECT id FROM executions WHERE source=?", fresh)[0][0]
            rejection = owner.svc.command("operator", "search", execution, owner.actor, "disabled-after-restart", json.dumps({"query": "must stay disabled"}), accepted=False)
            ctx.check("saved disabled search mode takes effect at restart", "native bridge disabled" in rejection)
            ctx.check("disabled search issues no additional provider request", old.wires.empty() and new.wires.empty())
        return {"receipt": receipt, "physical_search_requests": 2, "disabled_receipt": disabled_receipt}


def interrupted_embedding_configuration(ctx):
    with Recorder() as chat, Recorder("/v1/embeddings") as embedding:
        owner, env = setup(ctx, "profile-pending-embedding", chat)
        settings = owner.svc.data / "yuki.toml"
        settings.write_text(settings.read_text().replace('mode="lexical-only"', 'mode="hybrid"') +
                           '\n[memory.embedding]\nprovider="openai_compatible"\nmodel="embedding"\nbase_url=' + json.dumps(embedding.url.removesuffix("/embeddings")) + '\ndimensions=2\n')
        with owner.svc.running(environment=env):
            update = body(owner.op("profiles", "read"), "pending-embedding-recovered")
        kill_writer(ctx, owner.svc.data, "profile_crash_child", "profile-prepared-before-rename",
                    {"YUKI_PROFILE_TEST_BODY": json.dumps(update)}, sample="embedding")
        with owner.svc.running(environment=env):
            ctx.equal("pending configuration leaves operator reachable and pauses new embeddings", owner.op("memory", "vector-status")["embedding_enabled"], False)
            with urlopen("http://" + owner.svc.address + "/healthz", timeout=5) as response:
                health = json.load(response)
            ctx.equal("pending connection suspension is observable", (health["memory"]["degraded"], health["memory"]["reason"]), (True, "original_configuration_pending_restart"))
            receipt = owner.op("profiles", "apply", json.dumps(update))
            ctx.equal("recovered suspended connections explicitly require restart", receipt["effective_state"]["status"], "saved_pending_restart")
            ctx.check("pending boot buys no guessed embedding call", embedding.wires.empty())
        with owner.svc.running(environment=env):
            ctx.equal("settled configuration resumes enabled embedding consumer at restart", owner.op("memory", "vector-status")["mode"], "hybrid")
            ctx.equal("settled boot loads original recovered revision", owner.op("profiles", "read")["matches_loaded"], True)
        return receipt


def protocol_matrix(ctx):
    results = {}
    for provider, protocol, path in [
        ("openai", "chat_completions", "/v1/chat/completions"),
        ("deepseek", "responses", "/v1/responses"),
        ("openai", "responses", "/v1/responses"),
        ("custom_gateway", "responses", "/v1/responses"),
        ("anthropic", "anthropic_messages", "/v1/messages"),
        ("gemini", "gemini", "/v1/models/profile-wire:generateContent"),
    ]:
        label = provider + "-" + protocol
        with Recorder(path=path) as recorder:
            owner, env = setup(ctx, "profile-wire-" + label, recorder)
            # setup's original Chat URL is corrected before starting this independent owner.
            file = owner.svc.data / "model_profiles.toml"
            text = file.read_text()
            original_base = recorder.url.removesuffix("/chat/completions")
            base = "http://127.0.0.1:" + str(recorder.server.server_port) + "/v1"
            file.write_text(text.replace(original_base, base))
            with owner.svc.running(environment=env):
                view = owner.op("profiles", "read")
                update = body(view, "profile-wire")
                profile = update["document"]["profiles"]["primary"]
                profile.update(provider=provider, protocol=protocol)
                receipt = owner.op("profiles", "apply", json.dumps(update))
                ctx.equal(label + " applied through original operator entry", receipt["effective_state"]["status"], "applied")
                event = owner.human("Use the explicit configured protocol")
                paid, answer = owner.wire(recorder)
                if protocol == "chat_completions":
                    ctx.equal("OpenAI Chat profile preserves token dialect", paid["max_completion_tokens"], 128)
                    ctx.check("original token field was replaced", "max_tokens" not in paid)
                    reply(answer, "configured Chat result")
                elif protocol == "responses":
                    if provider == "deepseek":
                        ctx.check("fixed v4 DeepSeek profile keeps its original omissions",
                                  all(field not in paid for field in ("tool_choice", "store", "include")))
                    else:
                        ctx.equal(label + " retains original standard store flag", paid["store"], False)
                        ctx.check(label + " retains original encrypted reasoning request", "reasoning.encrypted_content" in paid["include"])
                    answer.put(({"status": "completed", "output": [{"type": "message", "id": "profile-msg",
                                 "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "configured Responses result"}]}]}, 0))
                elif protocol == "anthropic_messages":
                    ctx.equal("configured Messages output limit", paid["max_tokens"], 128)
                    ctx.check("disabled Messages thinking is omitted", "thinking" not in paid)
                    answer.put(({"type": "message", "role": "assistant", "stop_reason": "end_turn",
                                 "content": [{"type": "text", "text": "configured Messages result"}]}, 0))
                else:
                    ctx.equal("configured Gemini output limit", paid["generationConfig"]["maxOutputTokens"], 128)
                    answer.put(({"candidates": [{"finishReason": "STOP", "content": {"role": "model",
                                 "parts": [{"text": "configured Gemini result"}]}}]}, 0))
                owner.finished(event)
                results[label] = {"request_sha256": hashlib.sha256(json.dumps(paid, sort_keys=True).encode()).hexdigest(),
                                  "physical_attempts": owner.rows("SELECT count(*) FROM model_attempts")[0][0]}
                ctx.equal(label + " uses one physical attempt", results[label]["physical_attempts"], 1)
    return results


def work_profile(ctx):
    with Recorder() as original, Recorder() as changed:
        owner, env = setup(ctx, "profile-work", original)
        with owner.svc.running(environment=env):
            event = owner.human("Accept a Work and retain its paid profile")
            first, answer = owner.wire(original)
            reply(answer, calls=[call("profile-accept", "task_control", {"action": "accept", "goal": "original Work profile"})])
            _, answer = owner.wire(original)
            view = owner.op("profiles", "read")
            update = body(view, "new-work-profile")
            update["document"]["profiles"]["primary"]["base_url"] = changed.url.removesuffix("/chat/completions")
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "work-rotated-key"}
            owner.op("profiles", "apply", json.dumps(update))
            reply(answer, calls=[call("profile-child", "subagent_start", {"goal": "child follows original Work profile"})])
            a, qa = owner.wire(original)
            b, qb = owner.wire(original)
            if any(message.get("tool_call_id") == "profile-child" for message in a["messages"]):
                parent, parent_answer, child, child_answer = a, qa, b, qb
            else:
                parent, parent_answer, child, child_answer = b, qb, a, qa
            ctx.equal("existing Work parent retains paid model", parent["model"], first["model"])
            ctx.equal("new child of original Work hydrates original credential binding", child["model"], first["model"])
            ctx.check("original Work child never uses new model endpoint", changed.wires.empty())
            child_id = json.loads(next(message["content"] for message in reversed(parent["messages"])
                                       if message.get("tool_call_id") == "profile-child"))["result"]["child_id"]
            reply(child_answer, "Original private child result")
            owner.until(lambda: owner.rows("SELECT id FROM work_nodes WHERE parent IS NOT NULL AND status='completed'"))
            reply(parent_answer, calls=[call("profile-read-child", "task_control", {"action": "get", "work_id": child_id})])
            observed, parent_answer = owner.wire(original)
            ctx.equal("Work observes child mail with its original model", observed["model"], first["model"])
            ctx.check("original private result is consumed before settlement", "Original private child result" in str(observed["messages"][-1]))
            reply(parent_answer, calls=[call("profile-complete", "task_control", {"action": "complete", "result": "Original Work complete"})])
            owner.finished(event)
            contracts = owner.rows("SELECT profile_contract,count(*) FROM activations GROUP BY profile_contract")
            ctx.equal("Work and child share original immutable profile contract", len(contracts), 1)
            ctx.equal("original Work has two original activations", contracts[0][1], 2)
            return {"original_model": first["model"], "activation_contracts": contracts,
                    "actual_old_endpoint_requests": len(original.header_shapes)}


def revoked_prepared_operation(ctx):
    with Recorder() as recorder:
        owner, env = setup(ctx, "profile-prepared-revocation", recorder)
        token = env["PROFILE_OPERATOR_WRITER_A"]
        with owner.svc.running(environment=env):
            view = owner.op("profiles", "read")
        update = body(view,"revocation-recovered")
        original = file_hashes(owner)
        kill_writer(ctx, owner.svc.data, "profile_crash_child", "profile-prepared-before-rename",
                    {**env,"YUKI_PROFILE_TEST_TOKEN":token,"YUKI_PROFILE_TEST_BODY":json.dumps(update)},sample="revoked")
        audit = owner.rows("SELECT json_extract(value,'$.receipt.audit_id') FROM core_meta WHERE key LIKE 'model-profile-operation:%'")[0][0]
        registry = owner.svc.data / "operators.toml"
        approved = registry.read_text()
        registry.write_text(approved.replace('"control.config.file.content.read", "control.config.file.mutate"',
                                             '"control.config.file.content.read"',1))
        with owner.svc.running(environment=env):
            revoked_status, denied = wire(owner.svc.address,"/admin/v1/models/apply",token,update)
            ctx.equal("prepared operator rechecks current mutation grant",revoked_status,403)
            ctx.equal("revocation preserves both original files",file_hashes(owner),original)
            other_status, _ = wire(owner.svc.address,"/admin/v1/models/apply",env["PROFILE_OPERATOR_WRITER_B"],update)
            ctx.equal("another operator cannot take over original active operation",other_status,409)
            registry.write_text(approved)
            status, resumed = wire(owner.svc.address,"/admin/v1/models/apply",token,update)
            ctx.equal("restored original authority resumes its own operation",status,200)
            ctx.equal("recovery retains original audit identity",resumed["data"]["audit_id"],audit)
            ctx.equal("recovered operation applied once",resumed["data"]["success"],True)
            return {"revoked_status":revoked_status,"denial":denied["problem"]["code"],
                    "original_audit":audit,"resumed_receipt":resumed["data"]}


def run(ctx):
    built = subprocess.run([sys.executable, str(ctx.root / "ci/publish-contract.py"), "build",
                            "--out", str(ctx.output / "sdk-distributions"),
                            "--verify-work", str(ctx.working / "sdk")], capture_output=True, text=True)
    if built.returncode:
        ctx.write("sdk-failure.json", {"stderr": built.stderr})
    ctx.equal("candidate SDK distributions build and cross-verify", built.returncode, 0)
    schema = json.loads((ctx.root / "protocol/baseline-v4/admin-openapi.json").read_bytes())
    results = {}
    with Recorder() as original, Recorder() as changed:
        owner, environment = setup(ctx, "profile-normal", original)
        first = environment["PROFILE_OPERATOR_WRITER_A"]
        second = environment["PROFILE_OPERATOR_WRITER_B"]
        blocked = environment["PROFILE_OPERATOR_UNPRIVILEGED"]
        with owner.svc.running(environment=environment):
            def http(path="/admin/v1/models", token=first, payload=None, expected=200):
                status, value = wire(owner.svc.address, path, token, payload)
                ctx.equal("original Admin status " + path, status, expected)
                if status == 200:
                    ctx.check("fixed Admin response " + path,
                              matches_reply([schema], "POST" if payload else "GET", path, value))
                ctx.check("Admin response contains no private material",
                          all(secret not in json.dumps(value) for secret in
                              ("fixture-key-before", "fixture-key-after", "private-header-canary", first, second)))
                return value.get("data", value)

            http(token=None, expected=401)
            http(token="unknown-fixture", expected=401)
            owner.op("person", "set-admin", owner.actor, "true")
            http(token=blocked, expected=403)
            initial = http()
            ctx.equal("OS socket and bearer read same original revision",
                      owner.op("profiles", "read")["revision"], initial["revision"])
            ctx.check("retired route excluded from active document",
                      "conversation_compaction" not in initial["document"]["routes"])
            code = owner.op("personal", "issue", owner.actor, request_id(), "chat")["code"]
            status, device = wire(owner.svc.address, "/client/v1/pair", body={"code": code, "device_label": "profile fixture"})
            ctx.equal("actual chat device pairing", status, 200)
            chat_token = device["data"]["token"]
            http(token=chat_token)
            owner.op("person", "set-admin", owner.actor, "false")
            http(token=chat_token, expected=403)
            http(token=first)

            event = owner.human("Retain this original paid model chain")
            before_wire, old_answer = owner.wire(original)
            ctx.equal("original physical request selects old model", before_wire["model"], "before")
            update = body(initial, "after")
            update["document"]["profiles"]["primary"]["base_url"] = changed.url.removesuffix("/chat/completions")
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-after"}
            saved = http("/admin/v1/models/apply", payload=update)
            ctx.equal("successful hot configuration receipt", saved["effective_state"]["status"], "applied")
            after = http()
            ctx.check("bundle revision changed", after["revision"] != initial["revision"])
            ctx.equal("exact retry returns original durable receipt",
                      http("/admin/v1/models/apply", payload=update), saved)
            ctx.equal("model receipt preserves UUID spelling aliases",http("/admin/v1/models/apply",payload={**update,"request_id":update["request_id"].upper()}),saved)
            conflicting = copy.deepcopy(update)
            conflicting["document"]["profiles"]["primary"]["model"] = "conflicting-replay"
            http("/admin/v1/models/apply", payload=conflicting, expected=409)
            reply(old_answer, calls=[call("original-get", "get", {"target": "x"})])
            continued, answer = owner.wire(original)
            ctx.equal("already paid chain retains old endpoint and model", continued["model"], "before")
            reply(answer, "Original chain completed")
            owner.finished(event)
            event = owner.human("Use the newly applied profile")
            fresh, answer = owner.wire(changed)
            ctx.equal("new activation uses applied endpoint and model", fresh["model"], "after")
            reply(answer, "New chain completed")
            owner.finished(event)
            source = (owner.svc.data / "model_profiles.toml").read_text()
            for retained in ("# original deployment annotation", "# preserve this comment",
                             "untouched deployment value", "preserve-server-option", "private-header-canary", "retired-profile"):
                ctx.check("original TOML retains " + retained, retained in source)
            ctx.check("private key never stored in public TOML", "fixture-key-after" not in source)
            before = http()
            a, b = body(before, "winner-a"), body(before, "winner-b")
            with ThreadPoolExecutor(max_workers=2) as pool:
                futures = [pool.submit(wire, owner.svc.address, "/admin/v1/models/apply", token, payload)
                           for token, payload in [(first, a), (second, b)]]
                races = [future.result() for future in futures]
            ctx.equal("two operators with same revision produce one save and one conflict",
                      sorted(status for status, _ in races), [200, 409])
            current = http()
            ctx.check("winning model matches actual saved file",
                      current["document"]["profiles"]["primary"]["model"] in {"winner-a", "winner-b"})
            invalid = body(current, "invalid")
            invalid["document"]["profiles"]["primary"]["headers"] = {"Authorization": "forged"}
            hashes = file_hashes(owner)
            http("/admin/v1/models/apply", payload=invalid, expected=422)
            ctx.equal("rejected headers leave both files intact", file_hashes(owner), hashes)
            http("/admin/v1/operations/"+invalid["request_id"],expected=404)
            invalid=body(current,"unreferenced-key")
            invalid["api_keys"]={"UNREFERENCED_FOR_FIXTURE":"fixture-unreferenced-value"}
            http("/admin/v1/models/apply",payload=invalid,expected=422)
            failure_receipt=http("/admin/v1/operations/"+invalid["request_id"])
            ctx.equal("rejected model body has shared failed receipt",[failure_receipt["status"],failure_receipt["error_category"]],["failed","validation_error"])
            http("/admin/v1/models/apply",payload=invalid,expected=422)
            ctx.equal("model failure replay keeps its original metadata",http("/admin/v1/operations/"+invalid["request_id"]),failure_receipt)
            repaired=body(current,"repaired")
            repaired["request_id"]=invalid["request_id"]
            ctx.equal("repairing a failed model operation needs a new ID",http("/admin/v1/models/apply",payload=repaired,expected=409)["problem"]["code"],"request_conflict")
            for field, value in [("person_id", owner.actor), ("scope_type", "group")]:
                forged = body(current,"forged-scope")
                forged[field] = value
                http("/admin/v1/models/apply", payload=forged, expected=422)
            missing_revision = body(current,"missing-revision")
            del missing_revision["expected_revision"]
            http("/admin/v1/models/apply", payload=missing_revision, expected=422)
            if os.name != "nt":
                owner.svc.data.chmod(0o500)
                try:
                    http("/admin/v1/models/apply", payload=body(current, "read-only"), expected=422)
                    ctx.equal("read-only directory preserves files", file_hashes(owner), hashes)
                finally:
                    owner.svc.data.chmod(0o700)
            secret_update = {"request_id": request_id(), "expected_revision": current["revision"],
                             "name": "PROFILE_FIXTURE_KEY", "value": "fixture-key-final"}
            secret_receipt = http("/admin/v1/models/secrets", payload=secret_update)
            ctx.equal("secret-only exact replay retains receipt",
                      http("/admin/v1/models/secrets", payload=secret_update), secret_receipt)
            view = http()
            external = owner.svc.data / "model_profiles.toml"
            external.write_text(external.read_text() + "\n# external operator edit\n")
            external_hashes = file_hashes(owner)
            http("/admin/v1/models/apply", payload=body(view, "stale-external"), expected=409)
            ctx.equal("stale request cannot overwrite external edit", file_hashes(owner), external_hashes)
            view = http()
            restart = body(view, "pending-restart")
            restart["document"]["search"] = {"mode":"disabled"}
            receipt = http("/admin/v1/models/apply", payload=restart)
            ctx.equal("non-chat connection change requires restart", receipt["effective_state"]["status"], "saved_pending_restart")
            ctx.equal("saved pending document is not marked currently loaded", http()["matches_loaded"], False)
            event = owner.human("Retain live configuration until restart")
            live, answer = owner.wire(changed)
            ctx.equal("pending-restart save preserves live model", live["model"], view["document"]["profiles"]["primary"]["model"])
            reply(answer,"Current loaded profile retained")
            owner.finished(event)
            results["normal"] = {"hot_receipt": saved, "same_revision_statuses": [x[0] for x in races],
                                 "initial_revision": initial["revision"], "final_revision": http()["revision"]}

        mounted = Path("/yuki-profile-readonly/model_profiles.example.toml")
        results["read_only_mount"] = {"tested":False}
        if mounted.is_file():
            read_only, env = setup(ctx,"profile-read-only-mount",original)
            settings = read_only.svc.data / "yuki.toml"
            settings.write_text(settings.read_text().replace('model_profiles_file="model_profiles.toml"',
                                "model_profiles_file=" + json.dumps(str(mounted))))
            env.update(YUKI_PROFILE_BASE_URL=original.url.removesuffix("/chat/completions"),
                       YUKI_PROFILE_API_KEY="mount-fixture-key",YUKI_PROFILE_MODEL="mounted-original")
            original_hash = hashlib.sha256(mounted.read_bytes()).hexdigest()
            with read_only.svc.running(environment=env):
                view = read_only.op("profiles","read")
                ctx.equal("kernel read-only mount is reported non-writable",view["writable_directory"],False)
                failure = read_only.svc.command("operator","profiles","apply",json.dumps(body(view,"mount-change")),accepted=False)
                ctx.check("read-only mount save is rejected before staging","configuration_not_writable" in failure)
                ctx.equal("read-only mount contents remain unchanged",hashlib.sha256(mounted.read_bytes()).hexdigest(),original_hash)
                ctx.equal("read-only mount creates no pending file operation",read_only.rows("SELECT count(*) FROM core_meta WHERE key='model-profile-active-operation'"),[(0,)])
                ctx.equal("read-only mount preserves loaded revision",read_only.op("profiles","read")["matches_loaded"],True)
            results["read_only_mount"] = {"tested":True,"sha256":original_hash}

        cold, env = setup(ctx, "profile-paid-cold", original, pending=True)
        with ThreadPoolExecutor(max_workers=1) as pool:
            death = pool.submit(kill_writer, ctx, cold.svc.data, "compact_crash_child",
                                "profile-paid-response", env)
            paid, answer = cold.wire(original)
            ctx.equal("cold scenario initially pays original profile", paid["model"], "before")
            view = cold.op("profiles", "read")
            update = body(view, "cold-new-profile")
            update["document"]["profiles"]["primary"]["base_url"] = changed.url.removesuffix("/chat/completions")
            update["api_keys"] = {"PROFILE_FIXTURE_KEY": "cold-rotated-key"}
            cold.op("profiles", "apply", json.dumps(update))
            reply(answer, "Original paid response committed before abrupt stop")
            death.result()
        ctx.equal("physical response committed before process death",
                  cold.rows("SELECT state FROM model_attempts"), [("response",)])
        with cold.svc.running(environment=env):
            cold.finished(cold.pending_event)
            recovered = cold.rows("SELECT a.state,t.result FROM model_attempts a JOIN model_requests r ON r.id=a.logical JOIN activations v ON v.id=r.activation JOIN main_turns t ON t.execution=v.execution")
            ctx.write("cold-terminal-recovery.json", {"rows": recovered})
            ctx.equal("cold restart reuses the sole original terminal response", len(recovered), 1)
            ctx.equal("terminal response retains the original response state", recovered[0][0], "response")
            ctx.equal("cold completed result matches original paid text",
                      json.loads(recovered[0][1]), {"role":"assistant","content":"Original paid response committed before abrupt stop"})
            ctx.check("new endpoint is unused while recovering original paid response", changed.wires.empty())
            event = cold.human("New activation after cold recovery")
            fresh, answer = cold.wire(changed)
            ctx.equal("cold new activation uses applied profile", fresh["model"], "cold-new-profile")
            reply(answer, "Completed after cold recovery")
            cold.finished(event)
        results["paid_cold_recovery"] = {"original_terminal_response_reused": True, "original_physical_attempts": 1,
                                         "new_activation_model": fresh["model"]}

        for phase in ["profile-prepared-before-rename", "profile-secret-renamed", "profile-config-renamed"]:
            crash, env = setup(ctx, phase, original)
            with crash.svc.running(environment=env):
                view = crash.op("profiles", "read")
            operation = body(view, "recovered")
            operation["api_keys"] = {"PROFILE_FIXTURE_KEY": "fixture-key-recovered"}
            initial_hashes = file_hashes(crash)
            kill_writer(ctx, crash.svc.data, "profile_crash_child", phase,
                        {"YUKI_PROFILE_TEST_BODY": json.dumps(operation)})
            interrupted_hashes = file_hashes(crash)
            if phase == "profile-prepared-before-rename":
                ctx.equal("durable intent precedes original file writes", interrupted_hashes, initial_hashes)
            if phase == "profile-secret-renamed":
                ctx.equal("partial save still has original public file",
                          interrupted_hashes["model_profiles.toml"], initial_hashes["model_profiles.toml"])
                ctx.check("partial save records changed secret file",
                          interrupted_hashes["model_profiles.secrets.json"] != initial_hashes["model_profiles.secrets.json"])
            timestamps = {p.name: p.stat().st_mtime_ns for p in crash.svc.data.glob("model_profiles.*")}
            with crash.svc.running(environment=env):
                pending = crash.op("profiles", "receipt", operation["request_id"])
                ctx.equal("interrupted save retains original prepared receipt", pending["effective_state"]["status"], "prepared")
                control_pending = crash.op("control", "read", "operation.get", json.dumps({"request_id":operation["request_id"]}))
                ctx.equal("shared control reader keeps interrupted model save pending", control_pending["status"], "pending")
                ctx.equal("shared control reader preserves original expected revision", control_pending["expected_revision"], view["revision"])
                if phase == "profile-prepared-before-rename":
                    config = crash.svc.data / "model_profiles.toml"
                    original_bytes = config.read_bytes()
                    config.write_bytes(original_bytes + b"\n# independently changed after preparation\n")
                    conflict_hashes = file_hashes(crash)
                    failure = crash.svc.command("operator","profiles","apply",json.dumps(operation),accepted=False)
                    ctx.check("prepared operation detects original file mismatch", "version_conflict" in failure)
                    ctx.equal("mismatched external bytes are preserved", file_hashes(crash), conflict_hashes)
                    ctx.equal("conflicted operation retains prepared receipt", crash.op("profiles","receipt",operation["request_id"]), pending)
                    config.write_bytes(original_bytes)
                receipt = crash.op("profiles", "apply", json.dumps(operation))
                ctx.equal("same operation resumes its original two-file save", receipt["success"], True)
                control_done = crash.op("control", "read", "operation.get", json.dumps({"request_id":operation["request_id"]}))
                ctx.equal("recovered model save settles its original control operation", control_done["operation_id"], control_pending["operation_id"])
                ctx.equal("recovered control receipt uses actual model revision", (control_done["status"], control_done["result_revision"]), ("succeeded", receipt["revision"]))
                ctx.equal("recovered exact replay keeps original receipt",
                          crash.op("profiles", "apply", json.dumps(operation)), receipt)
                if phase == "profile-config-renamed":
                    ctx.equal("known renamed content is never renamed again",
                              {p.name: p.stat().st_mtime_ns for p in crash.svc.data.glob("model_profiles.*")}, timestamps)
                ctx.equal("recovered new runtime matches saved revision", crash.op("profiles", "read")["matches_loaded"], True)
            results[phase] = {"before": initial_hashes, "interrupted": interrupted_hashes,
                              "after": file_hashes(crash), "receipt": receipt}
    results["protocol_matrix"] = protocol_matrix(ctx)
    results["original_work_profile"] = work_profile(ctx)
    results["prepared_revocation"] = revoked_prepared_operation(ctx)
    results["auxiliary_task_routes"] = auxiliary_routes(ctx, ctx.working / "sdk")
    results["extraction_task_route"] = extraction_route(ctx)
    results["embedding_restart"] = embedding_restart(ctx)
    results["independent_search_profile"] = search_profile(ctx)
    results["interrupted_embedding_configuration"] = interrupted_embedding_configuration(ctx)
    binary = ctx.root / "target/debug" / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    return {"format_version": 1, "platform": os.name, "results": results,
            "physical_provider": "loopback-only", "paid_calls": 0, "real_extension_consumption": False,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "host_idl_sha256": hashlib.sha256((ctx.root / "sdk/rust/src/host-preview.json").read_bytes()).hexdigest(),
            "admin_openapi_sha256": hashlib.sha256((ctx.root / "protocol/baseline-v4/admin-openapi.json").read_bytes()).hexdigest(),
            "profile_metadata_sha256": hashlib.sha256((ctx.root / "protocol/admin/model-metadata.json").read_bytes()).hexdigest(),
            "validated_http_routes": ["GET /admin/v1/models", "POST /admin/v1/models/apply", "POST /admin/v1/models/secrets"]}
