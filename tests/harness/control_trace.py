"""Trace projections retain real model/effect states, with separate content authority."""
import hashlib
import json
import os
import sys
import uuid
from tests.harness.compact import Owner, call, reply
from tests.harness.control_config import wait
from tests.peers.chat.recorder import Recorder


def trace_peer(ctx, owner):
    folder = ctx.working / "trace-peer"
    folder.mkdir()
    args = [str(ctx.root / "tests/peers/control/invocation.py"), str(folder),
            str(ctx.control_python_sdk)]
    with (owner.svc.data / "yuki.toml").open("a") as config:
        config.write(f'''
[[plugins]]
id="trace.fixture"
namespace="trace.fixture"
program={json.dumps(sys.executable)}
args={json.dumps(args)}
grants=["command.invoke","control.invoke"]
commands=["/trace"]
''')
    return folder


def trace_forget(ctx, owner, folder, execution, original, continuation):
    wait(lambda: (folder / "ready").exists())
    reader = str(uuid.uuid4())
    owner.op("person", "create", reader)
    owner.op("person", "set-admin", reader, "true")
    account = owner.op("account", "open", "qq", "compact", "201", 1,
                       "--person", reader, "--start", 1)

    def send(name, value):
        path = folder / name
        stage = path.with_suffix(".tmp")
        stage.write_text(json.dumps(value), encoding="utf-8")
        stage.replace(path)

    def projection(serial, method="trace.read", content=True):
        # The private operator socket can be ready before Host negotiation.
        wait(lambda: owner.rows("SELECT 1 FROM host_installations "
                                "WHERE plugin='trace.fixture' AND enabled AND online"))
        path = folder / "command.json"
        path.unlink(missing_ok=True)
        event = owner.op("ingress", "record", json.dumps({
            "source": f"trace-reader-{serial}",
            "account": account["account"], "bot_stage": owner.bot["assignment"],
            "reply_bot": "100", "kind": "message",
            "occurred": 2000 + serial, "observed": 2000 + serial,
            "segments": [{"type": "text", "text": "/trace inspect original timeline"}],
        }), "--accept")["event"]
        try:
            command = wait(lambda: json.loads(path.read_bytes()))
        except AssertionError:
            ctx.write("trace-invocation-barrier.json", {
                "event": event,
                "event_state": owner.rows("SELECT state FROM host_command_events WHERE event=?", event),
                "plugin_state": owner.rows("SELECT enabled,online FROM host_installations WHERE plugin='trace.fixture'"),
                "bindings": owner.rows("SELECT path,plugin FROM host_commands"),
            })
            raise
        handle = command["params"]["handle"]
        send(f"request-{serial}.json", {
            "id": serial, "method": "control.read",
            "params": {"handle": handle, "method": method,
                       "args": {"kind": "turn", "identity": execution, "content": content}},
        })
        response = wait(lambda: json.loads((folder / f"response-{serial}.json").read_bytes()))
        return event, command, response.get("result", response.get("error"))

    def finish(command, data):
        send("reply.json", {"jsonrpc": "2.0", "id": command["id"],
                            "result": {"ok": True, "data": data}})

    def settled(command):
        operation = command["params"]["operation"]
        return owner.until(lambda: owner.rows(
            "SELECT c.result,o.result FROM host_calls c "
            "JOIN operations o ON o.namespace=c.execution AND o.operation_id=c.operation "
            "WHERE c.operation=? AND c.state='confirmed'", operation))[0]

    first_event, first, saved = projection(1)
    ctx.check("admin plugin reads the other person's original trace body",
              "a visible response" in json.dumps(saved))
    finish(first, saved)
    stored_before, _ = settled(first)
    ctx.check("authorized completed command retained its trace result",
              "a visible response" in stored_before)

    def artifact(event, descriptor, path):
        reader_execution = owner.rows("SELECT id FROM executions WHERE source=?", event)[0][0]
        return owner.svc.command("operator", "tools", "read-artifact", reader_execution, reader, json.dumps({
            "handle":descriptor["handle"],"version":descriptor["version"],"path":path,
        }))

    meta_event, meta_command, meta = projection(4, "trace.export", False)
    finish(meta_command, meta)
    settled(meta_command)
    raw, = owner.rows("SELECT body FROM tool_artifacts WHERE handle=?", meta["handle"])[0]
    document = json.loads(raw)["result"]
    ctx.equal("trace export uses versioned JSON bytes", hashlib.sha256(raw.encode()).hexdigest(), meta["version"])
    ctx.equal("trace export records its schema", document["schema"], "yuki.trace.export/1")
    ctx.check("metadata export omits all payloads",
              bool(document["page"]["items"]) and all("payload" not in x for x in document["page"]["items"]))
    ctx.equal("owner can read the exact version with the existing artifact entry point",
              artifact(meta_event, meta, "/result/schema")["value"], "yuki.trace.export/1")

    full_event, full_command, full = projection(5, "trace.export")
    send("request-51.json", {"id":51,"method":"control.read","params":{
        "handle":full_command["params"]["handle"],"method":"trace.export",
        "args":{"kind":"turn","identity":execution,"content":True},
    }})
    replay = wait(lambda: json.loads((folder / "response-51.json").read_bytes()))
    ctx.equal("same invocation repeats its original immutable export descriptor", replay["result"], full)
    send("request-52.json", {"id":52,"method":"control.read","params":{
        "handle":full_command["params"]["handle"],"method":"trace.export",
        "args":{"kind":"turn","identity":execution,"content":False},
    }})
    conflict = wait(lambda: json.loads((folder / "response-52.json").read_bytes()))
    ctx.equal("same invocation cannot substitute a different export selection", conflict["error"]["code"], -32003)
    finish(full_command, full)
    settled(full_command)
    body, = owner.rows("SELECT body FROM tool_artifacts WHERE handle=?", full["handle"])[0]
    ctx.check("full export omits secrets and transport URLs",
              "a visible response" in body and "PRIVATE-TRACE-" not in body and "SIGNED-URL" not in body)
    owner.op("person", "set-admin", reader, "false")
    exec_id = owner.rows("SELECT id FROM executions WHERE source=?", full_event)[0][0]
    refused = owner.svc.command("operator", "tools", "read-artifact", exec_id, reader,
                                json.dumps({"handle":full["handle"],"version":full["version"],"path":"/result/schema"}),
                                accepted=False)
    ctx.check("revoking operator rights removes original export read access", "forbidden" in refused)
    owner.op("person", "set-admin", reader, "true")
    second_event, second, delayed = projection(2)
    ctx.check("paused plugin holds the original trace before source erasure",
              "a visible response" in json.dumps(delayed))
    def accounting():
        return owner.rows(
            "SELECT id,state,json_extract(usage_json,'$.input'),"
            "json_extract(usage_json,'$.output'),json_extract(usage_json,'$.total') "
            "FROM model_attempts ORDER BY id"
        )

    attempts = accounting()
    ctx.check("the trace source has recorded model usage", any(row[2:] == (7, 3, 10)
                                                              for row in attempts))
    erasure = owner.op("privacy", "erase", "trace-continuation", owner.actor,
                       json.dumps([continuation]))
    ctx.check("erasure selected the original continuation and kept both reader commands",
              continuation in erasure["sources"] and
              not {original, first_event, second_event}.intersection(erasure["sources"]))
    finish(second, delayed)
    stored_after, effect = settled(second)
    ctx.equal("late Host trace result is empty after the other person's source erasure",
              stored_after, None)
    ctx.equal("late trace effect keeps the fact of settlement",
              json.loads(effect), {"erased": True})
    old, old_effect = settled(first)
    ctx.equal("previously persisted plugin trace response is erased", old, None)
    ctx.check("previously persisted trace effect lost its body",
              "a visible response" not in (old_effect or ""))
    ctx.equal("erasure clears both already published trace artifact bodies",
              owner.rows("SELECT body FROM tool_artifacts WHERE handle IN (?,?) ORDER BY handle",
                         meta["handle"],full["handle"]), [(None,),(None,)])
    _, unrelated, empty = projection(3)
    ctx.equal("the reader stays enabled and can see the missing timeline", empty["items"], [])
    finish(unrelated, empty)
    ctx.check("an independent query retains its result", settled(unrelated)[0] is not None)
    ctx.equal("trace erasure leaves original attempt counts and token usage unchanged",
              accounting(), attempts)
    return {"original_continuation": continuation, "reader_events": [first_event, second_event],
            "original_watermark": erasure["watermark"], "confirmed_effect_retained": True,
            "trace_export":{"schema":meta["schema"],"metadata_version":meta["version"],
                            "content_version":full["version"],"erased":True}}


def run(ctx, wire):
    with Recorder() as http:
        owner=Owner(ctx,'control-trace',20);owner.config(http)
        cfg=owner.svc.data/'yuki.toml';cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
        metadata='trace-metadata-fixture-0000000000000000000'
        content='trace-content-fixture-00000000000000000000'
        registry=owner.svc.data/'operators.toml'
        registry.write_text('[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="TRACE_METADATA"\ncapabilities=["control.execution.metadata.read"]\n[[operators]]\nprincipal_id="'+str(uuid.uuid4())+'"\ntoken_env="TRACE_CONTENT"\ncapabilities=["control.execution.metadata.read","control.execution.content.read"]\n')
        folder = trace_peer(ctx, owner)
        env={**os.environ,'TRACE_METADATA':metadata,'TRACE_CONTENT':content}
        with owner.svc.running(environment=env):
            event=owner.human('a public trace source')
            body,answer=owner.wire(http)
            execution=owner.rows('SELECT id FROM executions WHERE source=?',event)[0][0]
            reply(answer,calls=[call('trace-accept','task_control',{'action':'accept','goal':'retain trace continuation'})])
            body,answer=owner.wire(http)
            continuation=owner.human('follow-up trace input')
            owner.until(lambda:owner.rows('SELECT event FROM main_inputs WHERE execution=? AND event=?',execution,continuation))
            answer.put(({'choices':[{'message':{'role':'assistant','content':None,'tool_calls':[call('trace-get','get',{'target':json.dumps({'api_key':'PRIVATE-TRACE-ENCODED','nested':{'secret':'PRIVATE-TRACE-NESTED'}})})]},'finish_reason':'tool_calls'}],
                'provider_state':{'signature':'PRIVATE-TRACE-SIGNATURE','encrypted_content':'PRIVATE-TRACE-ENCRYPTED','thoughtSignature':'PRIVATE-TRACE-THOUGHT','api_key':'PRIVATE-TRACE-KEY','authorization':'PRIVATE-TRACE-AUTH','code':'PRIVATE-TRACE-PAIR','nested':[{'token':'PRIVATE-TRACE-TOKEN'}],'attachment_url':'https://fixture.invalid/download?token=SIGNED-URL'}},0))
            body,answer=owner.wire(http)
            ctx.check('follow-up input remains on original trace execution','follow-up trace input' in str(body['messages']))
            answer.put(({
                'choices': [{'message': {'role': 'assistant', 'content': 'a visible response'},
                             'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 7, 'completion_tokens': 3, 'total_tokens': 10},
            }, 0))
            owner.until(lambda: owner.rows("SELECT id FROM executions WHERE id=? AND state='completed'",execution))
            path=f'/admin/v1/traces/turn/{execution}'
            status,meta=wire(owner.svc.address,path,metadata)
            ctx.equal('metadata-only reader sees real trace timeline',status,200)
            ctx.check('trace contains actual model transitions',any(x['kind']=='model' for x in meta['data']['items']))
            ctx.check('metadata projection contains no payload',all('payload' not in x for x in meta['data']['items']))
            ctx.equal('metadata permission alone cannot read body',wire(owner.svc.address,path+'?content=true',metadata)[0],403)
            status,full=wire(owner.svc.address,path+'?content=true',content)
            ctx.equal('independent content permission reads body',status,200)
            ctx.write("trace-public-projection.json", full)
            raw=json.dumps(full)
            ctx.check('public response remains observable','a visible response' in raw)
            for secret in ['SIGNATURE','ENCRYPTED','THOUGHT','KEY','AUTH','PAIR','TOKEN','ENCODED','NESTED']:
                ctx.check('private trace '+secret+' absent','PRIVATE-TRACE-'+secret not in raw)
            ctx.check('opaque provider fields are explicitly omitted','opaque_provider_state' in raw)
            status,first=wire(owner.svc.address,path+'?limit=1',metadata)
            cursor=first['data']['next_cursor']
            ctx.check('trace has stable numeric cursor',isinstance(cursor,int))
            status,second=wire(owner.svc.address,path+'?after='+str(cursor),metadata)
            ctx.check('trace page does not repeat the first original entry',all(row['id']>cursor for row in second['data']['items']))
            original_ids=[v['id'] for v in meta['data']['items']]
            ctx.equal('event and original turn select same timeline',[v['id'] for v in wire(owner.svc.address,f'/admin/v1/traces/event/{event}',metadata)[1]['data']['items']],original_ids)
            ctx.equal('continuation event locates its actual original execution',[v['id'] for v in wire(owner.svc.address,f'/admin/v1/traces/event/{continuation}',metadata)[1]['data']['items']],original_ids)
        (folder / "ready").unlink(missing_ok=True)
        with owner.svc.running(environment=env):
            ctx.equal('cold restart preserves diagnostic cursor',[v['id'] for v in wire(owner.svc.address,path,metadata)[1]['data']['items']],original_ids)
            saved=registry.read_text();registry.write_text(saved.replace('token_env="TRACE_CONTENT"','token_env="TRACE_CONTENT"\nenabled=false'))
            ctx.equal('revoked observer cannot keep reading trace',wire(owner.svc.address,path+'?content=true',content)[0],409)
            registry.write_text(saved)
            plugin_erasure = trace_forget(ctx, owner, folder, execution, event, continuation)
            owner.op('privacy','forget','trace-forget',owner.actor)
            ctx.equal('original source erasure removes trace body and rows',wire(owner.svc.address,path+'?content=true',content)[1]['data']['items'],[])
    return {'execution':execution,'source_event':event,'original_trace_ids':original_ids,
            'opaque_material':'omitted','erasure':'verified','plugin_erasure':plugin_erasure}
