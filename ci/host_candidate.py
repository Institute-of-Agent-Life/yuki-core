"""Pack the exact native host and SDKs exercised by a successful CORE-032 run."""

import argparse
import json
import os
import platform
import shutil
import subprocess
import tempfile
import time
import zipfile
from contextlib import contextmanager
from pathlib import Path
from source import digest, source_identity

ROOT = Path(__file__).resolve().parents[1]


def cleanup_workspace(cleanup, timeout=5):
    deadline = time.monotonic() + timeout
    while True:
        try:
            cleanup()
            return
        except PermissionError as error:
            # A just-exited Windows executable can remain locked briefly.
            # This only retries removing our workspace, never the executed command.
            if getattr(error, "winerror", None) != 32 or time.monotonic() >= deadline:
                raise
            time.sleep(min(0.1, max(0, deadline - time.monotonic())))


@contextmanager
def portable_workspace():
    workspace = tempfile.mkdtemp(prefix="yuki-host-candidate-")
    try:
        yield workspace
    finally:
        # TemporaryDirectory retries unlink via directory recursion, hiding a
        # sharing violation on an executable behind Windows error 267. Keep
        # the original rmtree error for our bounded, cleanup-only retry.
        cleanup_workspace(lambda: shutil.rmtree(workspace))


def tested_contents(receipt, profile, personal=False, card=None):
    card = card or ("CORE-033" if personal else "CORE-032")
    personal = card == "CORE-033"
    media = card == "CORE-038"
    control = card == "CORE-037"
    importing = card == "CORE-057"
    history = card == "CORE-014"
    memory = card == "CORE-026"
    artifact = card == "CORE-015"
    members = card == "CORE-013"
    usage = card == "CORE-055"
    profiles = card == "CORE-048"
    notifications = card == "CORE-058"
    sessions = card == "CORE-051"
    maintenance = card == "CORE-035"
    participation = card == "CORE-036"
    auxiliary = card == "CORE-050"
    composition = card == "CORE-034"
    proof_name = {
        "CORE-057": "offline-config-reconciliation.json",
        "CORE-014": "original-history-wire.json",
        "CORE-015": "tool-result-pages.json",
        "CORE-026": "fact-evidence-acl.json",
        "CORE-013": "observed-members.json",
        "CORE-055": "usage-field-coverage.json",
        "CORE-037": "control-grants.json",
        "CORE-032": "host-delegation.json",
        "CORE-033": "personal-handoff.json",
        "CORE-038": "media-handle-bridge.json",
        "CORE-050": "aux-attempts.json",
        "CORE-036": "promotion-vs-self.json",
        "CORE-035": "maintenance-grants.json",
        "CORE-051": "isolated-session-replay.json",
        "CORE-048": "profile-apply-matrix.json",
        "CORE-058": "notification-publication.json",
        "CORE-034": "nested-boundary.json",
    }[card]
    proof = receipt(proof_name)
    data = json.loads(proof)
    sdk = json.loads(receipt("sdk-distributions/candidate.json"))
    binary = (
        ROOT / "target" / profile / ("yuki-core.exe" if os.name == "nt" else "yuki-core")
    )
    raw = binary.read_bytes()
    if digest(raw) != data["binary_sha256"]:
        raise ValueError("binary differs from exercised host")
    idl_path = (
        "sdk/rust/src/host-preview.json"
        if personal or media or composition or auxiliary or participation or maintenance or sessions or profiles or notifications or control or usage or importing or history or artifact or members or memory
        else "protocol/plugin/gateway/host-preview.json"
    )
    idl = (ROOT / idl_path).read_bytes()
    if digest(idl) != data["host_idl_sha256"]:
        raise ValueError("host interface differs from tested IDL")
    os_name = platform.system().lower()
    arch = (
        platform.machine()
        .lower()
        .replace("amd64", "x86_64")
        .replace("aarch64", "arm64")
    )
    files = {
        "bin/" + binary.name: raw,
        "protocol/host-preview.json": idl,
        "protocol/handshake.schema.json": (
            ROOT / "protocol/plugin/handshake.schema.json"
        ).read_bytes(),
        "evidence/" + proof_name: proof,
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "NOTICE": (ROOT / "NOTICE").read_bytes(),
        "licenses/Vec1.txt": (ROOT / "native/vec1/NOTICE").read_bytes(),
        "licenses/OpenCC.txt": (ROOT / "native/opencc/LICENSES.txt").read_bytes(),
    }
    if memory:
        proof=receipt('memory-restore.json')
        raw_fixture=(ROOT/'fixtures/memory/v4-restore.json').read_bytes()
        if digest(raw_fixture)!=json.loads(proof)['oracle_sha256']:raise ValueError('memory restore oracle differs from validated input')
        files['evidence/memory-restore.json']=proof
        files['evidence/v4-restore.json']=raw_fixture
        files['docs/memory-restore.md']=(ROOT/'docs/memory-restore.md').read_bytes()
        proof=receipt('memory-metadata.json')
        for name,expected in [('v4-metadata.json',json.loads(proof)['oracle_sha256']),('reader43-metadata.json',json.loads(proof)['upgrade']['fixture_sha256'])]:
            raw_fixture=(ROOT/'fixtures/memory'/name).read_bytes()
            if digest(raw_fixture)!=expected:raise ValueError('memory metadata fixture differs from validated input')
            files['evidence/'+name]=raw_fixture
        files['evidence/memory-metadata.json']=proof
        files['docs/memory-metadata.md']=(ROOT/'docs/memory-metadata.md').read_bytes()
        proof=receipt('memory-selectors.json')
        original=(ROOT/'fixtures/memory/v4-selectors.json').read_bytes()
        if digest(original)!=json.loads(proof)['oracle_sha256']:raise ValueError('memory selector oracle differs from validated input')
        files['evidence/v4-selectors.json']=original
        files['evidence/memory-selectors.json']=proof
        files['docs/memory-selectors.md']=(ROOT/'docs/memory-selectors.md').read_bytes()
        proof=receipt('memory-temporal.json')
        original=(ROOT/'fixtures/memory/v4-temporal.json').read_bytes()
        if digest(original)!=json.loads(proof)['oracle_sha256']:raise ValueError('memory temporal oracle differs from validated input')
        files['evidence/v4-temporal.json']=original
        files['evidence/memory-temporal.json']=proof
        old=(ROOT/'fixtures/memory/reader44-temporal.json').read_bytes()
        if digest(old)!=json.loads(proof)['upgrade']['fixture_sha256']:raise ValueError('reader 44 temporal upgrade fixture differs from validated input')
        files['evidence/reader44-temporal.json']=old
        files['docs/memory-temporal.md']=(ROOT/'docs/memory-temporal.md').read_bytes()
    if importing:
        files["docs/model-config-import.md"]=(ROOT/"docs/model-config-import.md").read_bytes()
        files["evidence/legacy-import-provenance.json"]=(ROOT/"tests/fixtures/import/model-v4/provenance.json").read_bytes()
    if members:
        raw_fixture=(ROOT/'fixtures/social/v4-members.json').read_bytes()
        if digest(raw_fixture)!=data['members_oracle_sha256']:raise ValueError('member contract differs from verified original')
        files['evidence/v4-members.json']=raw_fixture
        files['docs/observed-members-compat.md']=(ROOT/'docs/observed-members-compat.md').read_bytes()
        recall=receipt('original-recall-wire.json')
        original=(ROOT/'fixtures/social/v4-recall.json').read_bytes()
        if digest(original)!=json.loads(recall)['recall_oracle_sha256']:raise ValueError('recall contract differs from verified original')
        files['evidence/original-recall-wire.json']=recall
        files['evidence/v4-recall.json']=original
        files['docs/recall-compat.md']=(ROOT/'docs/recall-compat.md').read_bytes()
    if artifact:
        isolated=receipt('tool-call-isolation.json')
        original_rejections=(ROOT/'fixtures/tool-results/v4-call-rejections.json').read_bytes()
        if digest(original_rejections)!=json.loads(isolated)['oracle_sha256']:raise ValueError('call rejection oracle differs from exercised source')
        files['evidence/tool-call-isolation.json']=isolated
        files['docs/tool-call-isolation.md']=(ROOT/'docs/tool-call-isolation.md').read_bytes()
        files['evidence/v4-call-rejections.json']=original_rejections
        views=receipt('original-tool-views.json')
        original=(ROOT/'fixtures/tool-results/v4-tool-views.json').read_bytes()
        if digest(original)!=json.loads(views)['oracle_sha256']:raise ValueError('tool views differ from verified original')
        files['evidence/original-tool-views.json']=views
        files['evidence/v4-tool-views.json']=original
        files['docs/original-tool-views.md']=(ROOT/'docs/original-tool-views.md').read_bytes()
        for path,key in [("fixtures/tool-results/v4-original-declaration.json","artifact_declaration_sha256"),("fixtures/tool-results/v4-original-pages.json","oracle_sha256")]:
            raw_fixture=(ROOT/path).read_bytes()
            if digest(raw_fixture)!=data[key]:raise ValueError("artifact contract differs from verified original")
            files["evidence/"+Path(path).name]=raw_fixture
        files["docs/tool-artifact-compat.md"]=(ROOT/"docs/tool-artifact-compat.md").read_bytes()
    if history:
        declaration=(ROOT/"fixtures/history/v4-query-declaration.json").read_bytes()
        if digest(declaration)!=data["history_declaration_sha256"]:raise ValueError("history declaration differs from tested original")
        files["protocol/query_chat_history.json"]=declaration
        files["evidence/v4-original-query.json"]=(ROOT/"fixtures/history/v4-original-query.json").read_bytes()
    if usage:
        contract=(ROOT/"protocol/control/usage.json").read_bytes()
        if digest(contract)!=data["usage_schema_sha256"]:raise ValueError("usage contract differs from verified source")
        files["protocol/control/usage.json"]=contract
        for path in ("protocol/control/README.md","protocol/plugin/control/methods.json","protocol/admin/capabilities.json","protocol/admin/person-capabilities.json"):
            files[path]=(ROOT/path).read_bytes()
    if control:
        permissions=receipt('original-permission-directory.json')
        original=(ROOT/'fixtures/control/v4-permissions.json').read_bytes()
        if digest(original)!=json.loads(permissions)['permission_oracle_sha256']:raise ValueError('permission directory differs from verified original')
        files['evidence/original-permission-directory.json']=permissions
        files['evidence/v4-permissions.json']=original
        files['docs/permission-directory.md']=(ROOT/'docs/permission-directory.md').read_bytes()
        for path,expected in [("protocol/plugin/control/methods.json","control_schema_sha256"),("protocol/baseline-v4/admin-openapi.json","admin_openapi_sha256")]:
            raw_contract=(ROOT/path).read_bytes()
            if digest(raw_contract)!=data[expected]:raise ValueError("control contract differs from verified source")
            files[path]=raw_contract
        for path in ("protocol/admin/capabilities.json","protocol/admin/person-capabilities.json","protocol/plugin/control/README.md"):
            files[path]=(ROOT/path).read_bytes()
    if profiles:
        for path, expected in [("protocol/baseline-v4/admin-openapi.json", "admin_openapi_sha256"),
                               ("protocol/admin/model-metadata.json", "profile_metadata_sha256")]:
            member = (ROOT / path).read_bytes()
            if digest(member) != data[expected]:
                raise ValueError("model configuration contract differs from verified source")
            files[path] = member
        files["config/model_profiles.example.toml"] = (ROOT / "config/model_profiles.example.toml").read_bytes()
        files["protocol/admin/capabilities.json"] = (ROOT / "protocol/admin/capabilities.json").read_bytes()
        files["docs/model-profiles.md"] = (ROOT / "docs/model-profiles.md").read_bytes()
    if notifications:
        methods=(ROOT/"protocol/plugin/notifications/methods.json").read_bytes()
        if digest(methods)!=data["notification_schema_sha256"] or set(data["negotiated_notification_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("notification methods differ from exercised original")
        files["protocol/notifications/methods.json"]=methods
    if sessions:
        methods=(ROOT/"protocol/plugin/sessions/methods.json").read_bytes()
        if digest(methods)!=data["session_schema_sha256"] or set(data["negotiated_session_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("isolated session methods differ from exercised original")
        files["protocol/sessions/methods.json"]=methods
    if maintenance:
        methods=(ROOT/"protocol/plugin/maintenance/methods.json").read_bytes()
        if digest(methods)!=data["maintenance_schema_sha256"] or set(data["negotiated_maintenance_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("maintenance methods differ from exercised original")
        files["protocol/maintenance/methods.json"]=methods
    if participation:
        methods=(ROOT/"protocol/plugin/participation/methods.json").read_bytes()
        if digest(methods)!=data["participation_schema_sha256"] or set(data["negotiated_participation_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("participation methods differ from exercised original")
        files["protocol/participation/methods.json"]=methods
    if auxiliary:
        methods=(ROOT/"protocol/plugin/aux-model/methods.json").read_bytes()
        if digest(methods)!=data["aux_schema_sha256"] or set(data["negotiated_aux_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("auxiliary methods differ from exercised original")
        files["protocol/aux-model/methods.json"]=methods
    if composition:
        methods=(ROOT/"protocol/plugin/composition/methods.json").read_bytes()
        if digest(methods)!=data["composition_schema_sha256"] or set(data["negotiated_composition_methods"])!=set(json.loads(methods)["methods"]):
            raise ValueError("composition methods differ from exercised original")
        files["protocol/composition/methods.json"]=methods
        files["evidence/vm-recovery-cuts.json"]=receipt("vm-recovery-cuts.json")
    if media:
        methods = (ROOT / "protocol/plugin/media/methods.json").read_bytes()
        if digest(methods) != data["media_schema_sha256"] or set(
            data["negotiated_media_methods"]
        ) != set(json.loads(methods)["methods"]):
            raise ValueError("media methods differ from exercised original")
        files["protocol/media/methods.json"] = methods
        gateway=receipt("gateway-inbound-media.json")
        doc=json.loads(gateway)
        oracle=(ROOT/"fixtures/media/gateway-media-v4.json").read_bytes()
        if digest(oracle)!=doc["original_oracle_sha256"] or data["gateway_inbound"]!=doc:
            raise ValueError("inbound gateway receipt differs from validated source")
        files["evidence/gateway-inbound-media.json"]=gateway
        files["evidence/gateway-media-v4.json"]=oracle
        files["docs/gateway-inbound-media.md"]=(ROOT/"docs/gateway-inbound-media.md").read_bytes()
    if personal:
        methods = (ROOT / "protocol/plugin/personal/methods.json").read_bytes()
        if digest(methods) != data["personal_schema_sha256"] or set(
            data["negotiated_methods"]
        ) != set(json.loads(methods)["methods"]):
            raise ValueError("personal interface differs from exercised methods")
        files["protocol/personal/methods.json"] = methods
        proof=receipt('connector-authentication.json')
        original=(ROOT/'fixtures/personal/connector-identity.json').read_bytes()
        if digest(original)!=json.loads(proof)['original_oracle_sha256']:raise ValueError('connector identity oracle differs from validated input')
        files['evidence/connector-authentication.json']=proof
        files['evidence/connector-identity.json']=original
        selected=receipt('client-entry-selection.json')
        oracle=(ROOT/'fixtures/personal/entry-selection.json').read_bytes()
        if digest(oracle)!=json.loads(selected)['original_oracle_sha256']:
            raise ValueError('Client9 selection oracle differs from validated input')
        files['evidence/client-entry-selection.json']=selected
        files['evidence/entry-selection.json']=oracle
        files['docs/client-entry-selection.md']=(ROOT/'docs/client-entry-selection.md').read_bytes()
        uploads=receipt('client-upload-handoff.json')
        oracle=(ROOT/'fixtures/personal/client-uploads-v4.json').read_bytes()
        if digest(oracle)!=json.loads(uploads)['original_oracle_sha256']:
            raise ValueError('client upload oracle differs from validated input')
        files['evidence/client-upload-handoff.json']=uploads
        files['evidence/client-uploads-v4.json']=oracle
        files['docs/client-upload-receipts.md']=(ROOT/'docs/client-upload-receipts.md').read_bytes()
        stream=receipt('client-outbound-stream.json')
        source=(ROOT/'fixtures/personal/client-stream-v4.json').read_bytes()
        if digest(source)!=json.loads(stream)['original_oracle_sha256']:
            raise ValueError('Client9 stream oracle differs from validated input')
        files['evidence/client-outbound-stream.json']=stream
        files['evidence/client-stream-v4.json']=source
        files['docs/client-stream.md']=(ROOT/'docs/client-stream.md').read_bytes()

        files['docs/connector-authentication.md']=(ROOT/'docs/connector-authentication.md').read_bytes()
        for index in (
            "protocol/personal/source.json",
            "protocol/connector/4/source.json",
        ):
            index_raw = (ROOT / index).read_bytes()
            files[index] = index_raw
            for member in json.loads(index_raw)["members"]:
                name = member["path"]
                blob = (ROOT / name).read_bytes()
                if digest(blob) != member["sha256"]:
                    raise ValueError("fixed wire changed: " + name)
                files[name] = blob
    target_info = subprocess.check_output(["rustc", "-vV"], text=True)
    host = next(
        line.split(": ", 1)[1]
        for line in target_info.splitlines()
        if line.startswith("host: ")
    )
    if os.name == "nt" and "windows-gnu" in host:
        runtime = ROOT / "native/windows-runtime"
        prov = json.loads((runtime / "provenance.json").read_bytes())
        compiler = Path(os.environ.get("YUKI_MINGW_BIN", "")) / "g++.exe"
        version = subprocess.check_output(
            [str(compiler), "-dumpfullversion"], text=True
        ).strip()
        if prov["compiler"] != version:
            raise ValueError("missing runtime notices for Windows compiler " + version)
        for name, witness in prov["members"].items():
            raw_notice = (runtime / name).read_bytes()
            if digest(raw_notice) != witness["sha256"]:
                raise ValueError("runtime license digest mismatch")
            files["licenses/GCC/" + name] = raw_notice
        files["licenses/GCC/provenance.json"] = (
            runtime / "provenance.json"
        ).read_bytes()
    for role in ("python-sdk", "rust-sdk", "golden", "verification"):
        item = sdk["members"][role]
        n = item["name"]
        if Path(n).name != n:
            raise ValueError("unsafe distribution member")
        content = receipt("sdk-distributions/" + n)
        if digest(content) != item["sha256"]:
            raise ValueError("SDK digest mismatch")
        files["sdk/" + n] = content
    with portable_workspace() as sandbox:
        # The verified binary must also start after transfer without the original compiler or its PATH.
        exe = Path(sandbox) / binary.name
        exe.write_bytes(raw)
        exe.chmod(0o755)
        env = {
            "PATH": (
                os.path.join(os.environ.get("SystemRoot", "C:\\Windows"), "System32")
                if os.name == "nt"
                else "/usr/bin:/bin"
            )
        }
        if os.name == "nt":
            env["SystemRoot"] = os.environ["SystemRoot"]
        directory = Path(sandbox) / "data"
        for command in ("init", "doctor"):
            result = subprocess.run(
                [str(exe), "--data", str(directory), command, *(["--memory-mode","lexical-only"] if command=="init" else [])],
                cwd=sandbox,
                env=env,
                capture_output=True,
                timeout=30,
            )
            if result.returncode:
                raise RuntimeError(
                    f"portable host failed {command} ({result.returncode}): "
                    + result.stderr.decode(errors="replace")[:200]
                )
            json.loads(result.stdout)
    return files, os_name, arch, host, sdk, idl


def build(manifest_path, out, development=False):
    base = manifest_path.parent
    m = json.loads(manifest_path.read_bytes())
    source = source_identity(ROOT)
    if (
        m["card"] not in {"CORE-026", "CORE-013", "CORE-015", "CORE-014", "CORE-032", "CORE-033", "CORE-034", "CORE-038", "CORE-050", "CORE-036", "CORE-035", "CORE-051", "CORE-048", "CORE-058", "CORE-037", "CORE-055", "CORE-057"}
        or m["status"] != "verified"
        or m["exit_code"] != 0
        or not m["assertions"]
        or not all(a["passed"] for a in m["assertions"])
    ):
        raise ValueError("passing host capability manifest required")
    if m["source"] != source or (source["dirty"] and not development):
        raise ValueError("candidate needs matching source identity and clean commit")

    def receipt(name):
        raw = (base / name).read_bytes()
        if digest(raw) != m["artifacts"].get(name):
            raise ValueError("unverified member " + name)
        return raw

    personal = m["card"] == "CORE-033"
    media = m["card"] == "CORE-038"
    control = m["card"] == "CORE-037"
    importing = m["card"] == "CORE-057"
    history = m["card"] == "CORE-014"
    memory = m["card"] == "CORE-026"
    artifact = m["card"] == "CORE-015"
    observed_members = m["card"] == "CORE-013"
    usage = m["card"] == "CORE-055"
    profiles = m["card"] == "CORE-048"
    notifications = m["card"] == "CORE-058"
    sessions = m["card"] == "CORE-051"
    maintenance = m["card"] == "CORE-035"
    participation = m["card"] == "CORE-036"
    auxiliary = m["card"] == "CORE-050"
    composition = m["card"] == "CORE-034"
    proof_name = {
        "CORE-057": "offline-config-reconciliation.json",
        "CORE-014": "original-history-wire.json",
        "CORE-015": "tool-result-pages.json",
        "CORE-026": "fact-evidence-acl.json",
        "CORE-013": "observed-members.json",
        "CORE-055": "usage-field-coverage.json",
        "CORE-037": "control-grants.json",
        "CORE-032": "host-delegation.json",
        "CORE-033": "personal-handoff.json",
        "CORE-038": "media-handle-bridge.json",
        "CORE-050": "aux-attempts.json",
        "CORE-036": "promotion-vs-self.json",
        "CORE-035": "maintenance-grants.json",
        "CORE-051": "isolated-session-replay.json",
        "CORE-048": "profile-apply-matrix.json",
        "CORE-058": "notification-publication.json",
        "CORE-034": "nested-boundary.json",
    }[m["card"]]
    data = json.loads(receipt(proof_name))
    files, os_name, arch, host, sdk, idl = tested_contents(receipt, "debug", card=m["card"])
    target = (
        (
            {
                "CORE-057": "yuki-model-import-preview-",
                "CORE-014": "yuki-history-preview-",
                "CORE-015": "yuki-artifact-preview-",
                "CORE-026": "yuki-memory-preview-",
                "CORE-013": "yuki-social-preview-",
                "CORE-055": "yuki-usage-preview-",
                "CORE-037": "yuki-control-preview-",
                "CORE-032": "yuki-host-preview-",
                "CORE-033": "yuki-personal-preview-",
                "CORE-038": "yuki-media-preview-",
                "CORE-050": "yuki-aux-model-preview-",
                "CORE-036": "yuki-participation-preview-",
                "CORE-035": "yuki-maintenance-preview-",
                "CORE-051": "yuki-session-preview-",
                "CORE-058": "yuki-notification-preview-",
                "CORE-034": "yuki-code-preview-",
                "CORE-048": "yuki-profile-preview-",
            }[m["card"]]
        )
        + source["commit"][:12]
        + "-"
        + os_name
        + "-"
        + arch
    )
    members = {
        n: {"sha256": digest(content), "bytes": len(content)}
        for n, content in sorted(files.items())
    }
    candidate = {
        "status": "unpublished-development-candidate",
        "source_commit": source["commit"],
        "source_tree_sha256": source["tree_sha256"],
        "dirty": source["dirty"],
        "os": os_name,
        "arch": arch,
        "binary_profile": "tested debug",
        "rust_host": host,
        "host_schema_revision": 2,
        "host_schema_sha256": digest(idl),
        "handshake_schema_revision": 1,
        "sdk_versions": sdk["sdk_versions"],
        "implemented_methods": sorted(json.loads(idl)["methods"]),
        "validation": {
            "card": m["card"],
            "platform": m["platform"],
            "assertions": len(m["assertions"]),
            "manifest_sha256": digest(manifest_path.read_bytes()),
            "clean_path_launch": "verified",
            "consumer": "synthetic SDK subprocesses",
            "stable": False,
        },
        "members": members,
    }
    if importing:
        candidate["legacy_input_schema_revision"]="0156"
        candidate["validated_cli"]=["import-models --inspect","import-models","import-models --resume"]
        candidate["database_schema_revision"]=None
        candidate["validation"]["consumer"]="offline stopped-copy importer; no runtime owner or provider started"
        candidate["validation"]["real_extension_consumers"]=False
    if memory:
        candidate['database_schema_revision']=m['schema_version']
        candidate['validated_core_tools']=['memory_change','read_memory']
        candidate['validated_memory_operations']=['create','correct','invalidate','restore','update_metadata']
        candidate['validated_memory_selectors']=['current_speaker/person']
        candidate['memory_selector_oracle_sha256']=json.loads(receipt('memory-selectors.json'))['oracle_sha256']
        candidate['memory_temporal_oracle_sha256']=json.loads(receipt('memory-temporal.json'))['oracle_sha256']
        candidate['validated_memory_temporal_fields']=['valid_from','valid_until']
        candidate['memory_metadata_oracle_sha256']=json.loads(receipt('memory-metadata.json'))['oracle_sha256']
        candidate['memory_restore_oracle_sha256']=json.loads(receipt('memory-restore.json'))['oracle_sha256']
        candidate['validation']['consumer']='physical Main, QQ command and original writer fixtures; SDK packages independently installed'
        candidate['validation']['real_extension_consumers']=False
    if observed_members:
        candidate['database_schema_revision']=m['schema_version']
        candidate['validated_core_tools']=['get_group_members','recall_own_message']
        candidate['recall_oracle_sha256']=json.loads(receipt('original-recall-wire.json'))['recall_oracle_sha256']
        candidate['members_oracle_sha256']=data['members_oracle_sha256']
        candidate['coverage']='observed'
        candidate['validation']['consumer']='physical Main, authenticated original QQ recall and observed directory fixtures; SDK packages independently installed'
        candidate['validation']['real_extension_consumers']=False
    if artifact:
        candidate["database_schema_revision"]=m["schema_version"]
        candidate["artifact_declaration_sha256"]=data["artifact_declaration_sha256"]
        candidate["validated_core_tools"]=["read_tool_artifact","lookup_tools","time_get_current","update_short_state"]
        candidate["tool_views_oracle_sha256"]=json.loads(receipt("original-tool-views.json"))["oracle_sha256"]
        candidate["validated_call_isolation_protocols"]=[p["protocol"] for p in json.loads(receipt("tool-call-isolation.json"))["protocols"]]
        candidate["validated_artifact_operations"]=data["validated_operations"]
        candidate["validation"]["consumer"]="physical Main tool continuation and fixed original JSON/text page fixtures; SDK packages independently installed"
        candidate["validation"]["real_extension_consumers"]=False
        candidate["native_image_artifacts_validated"]=False
    if history:
        candidate["database_schema_revision"]=m["schema_version"]
        candidate["history_declaration_sha256"]=data["history_declaration_sha256"]
        candidate["validated_core_tools"]=["query_chat_history"]
        candidate["validation"]["consumer"]="physical Main Chat tool invocation and fixed v4 query fixtures; SDK packages independently installed"
        candidate["validation"]["real_extension_consumers"]=False
    if usage:
        candidate["usage_schema_revision"]=1
        candidate["usage_schema_sha256"]=data["usage_schema_sha256"]
        candidate["database_schema_revision"]=data["database_schema_revision"]
        candidate["validated_http_routes"]=["GET /admin/v1/models/stats"]
        candidate["validation"]["consumer"]="native Admin HTTP and original billing ledger fixtures"
        candidate["validation"]["real_extension_consumers"]="pending EXT-049"
    if control:
        candidate['validated_core_tools']=['get_my_capabilities']
        candidate['permission_oracle_sha256']=json.loads(receipt('original-permission-directory.json'))['permission_oracle_sha256']
        candidate["control_schema_revision"]=1
        candidate["control_schema_sha256"]=data["control_schema_sha256"]
        candidate["admin_openapi_sha256"]=data["admin_openapi_sha256"]
        candidate["database_schema_revision"]=data["database_schema_revision"]
        candidate["validated_control_methods"]=data["validated_control_methods"]
        candidate["validated_action_families"]=data["validated_action_families"]
        candidate["validation"]["consumer"]="installed Python wheel and external Rust crate consumer; native Admin and OS operator"
        candidate["validation"]["real_extension_consumers"]="pending EXT-022, EXT-037, EXT-038, EXT-039 and EXT-049"
    if profiles:
        candidate["model_profile_schema_revision"] = 3
        candidate["admin_openapi_sha256"] = data["admin_openapi_sha256"]
        candidate["validated_http_routes"] = data["validated_http_routes"]
        candidate["validated_profile_protocols"] = sorted(data["results"]["protocol_matrix"])
        candidate["validated_model_tasks"] = ["chat_agent", "memory_extraction", *sorted(data["results"]["auxiliary_task_routes"])]
        candidate["validated_runtime_connections"] = ["independent_search_profile", "embedding_restart"]
        candidate["validation"]["consumer"] = "native operator and fixed Admin HTTP fixtures"
        candidate["validation"]["real_extension_consumers"] = "pending EXT-049"
    if personal:
        candidate["personal_schema_revision"] = 1
        selected=json.loads(receipt('client-entry-selection.json'))
        candidate['validated_client_selection_headers']=selected['selected_headers']
        candidate['client_selection_oracle_sha256']=selected['original_oracle_sha256']
        stream=json.loads(receipt('client-outbound-stream.json'))
        candidate['client_stream_oracle_sha256']=stream['original_oracle_sha256']
        candidate['client_upload_oracle_sha256']=json.loads(receipt('client-upload-handoff.json'))['original_oracle_sha256']
        candidate['validated_stream_recovery']='fresh sync and durable events after cursor'
        candidate['database_schema_revision']=m['schema_version']
        candidate["personal_schema_sha256"] = data["personal_schema_sha256"]
        candidate["client_api_revision"] = 9
        candidate["connector_wire_revision"] = 4
        connector=json.loads(receipt('connector-authentication.json'))
        candidate['connector_identity_oracle_sha256']=connector['original_oracle_sha256']
        candidate['validated_connector_methods']=connector['methods']
        candidate["validated_personal_methods"] = sorted(data["negotiated_methods"])
        candidate["validated_http_routes"] = data["validated_routes"]
    if media:
        candidate["media_schema_revision"] = 1
        candidate["database_schema_revision"]=m["schema_version"]
        candidate["validated_gateway_media_method"]=data["gateway_inbound"]["method"]
        candidate["gateway_media_oracle_sha256"]=data["gateway_inbound"]["original_oracle_sha256"]
        candidate["media_schema_sha256"] = data["media_schema_sha256"]
        candidate["validated_media_methods"] = sorted(data["negotiated_media_methods"])
    if notifications:
        candidate["notification_schema_revision"]=1
        candidate["notification_schema_sha256"]=data["notification_schema_sha256"]
        candidate["validated_notification_methods"]=data["negotiated_notification_methods"]
        candidate["validation"]["real_extension_consumers"]="pending EXT-027 and EXT-047"
    if sessions:
        candidate["session_schema_revision"]=1
        candidate["session_schema_sha256"]=data["session_schema_sha256"]
        candidate["validated_session_methods"]=data["negotiated_session_methods"]
        candidate["validated_session_protocols"]=["chat"]+[item["protocol"] for item in data["native_wires"]]
        candidate["validation"]["real_extension_consumers"]="pending EXT-047"
    if maintenance:
        candidate["maintenance_schema_revision"]=1
        candidate["maintenance_schema_sha256"]=data["maintenance_schema_sha256"]
        candidate["validated_maintenance_methods"]=data["negotiated_maintenance_methods"]
        candidate["validation"]["real_extension_consumers"]="pending EXT-033, EXT-034, EXT-035, EXT-036 and EXT-048"
    if participation:
        candidate["participation_schema_revision"]=1
        candidate["participation_schema_sha256"]=data["participation_schema_sha256"]
        candidate["validated_participation_methods"]=data["negotiated_participation_methods"]
        candidate["validation"]["real_extension_consumer"]="pending EXT-025"
    if auxiliary:
        candidate["aux_schema_revision"]=1
        candidate["aux_schema_sha256"]=data["aux_schema_sha256"]
        candidate["validated_aux_methods"]=data["negotiated_aux_methods"]
        candidate["validated_aux_protocols"]=["chat"]+[item["dialect"] for item in data["wires"]]
        candidate["validated_media_protocols"]=["chat"]
        candidate["validation"]["real_extension_consumers"]="pending EXT-025, EXT-030, EXT-032, EXT-033 and EXT-035"
    if composition:
        candidate["composition_schema_revision"]=1
        candidate["composition_schema_sha256"]=data["composition_schema_sha256"]
        candidate["validated_composition_methods"]=data["negotiated_composition_methods"]
        candidate["validation"]["consumer"]="opaque SDK checkpoint fixtures"
        candidate["validation"]["real_vm_consumer"]="pending EXT-023 and EXT-024"
    files["candidate.json"] = (
        json.dumps(candidate, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    ).encode()
    out.mkdir(parents=True, exist_ok=False)
    zip_path = out / (target + ".zip")
    with zipfile.ZipFile(zip_path, "x") as archive:
        for name, content in sorted(files.items()):
            entry = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            entry.create_system = 3
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (
                0o100755 if name.startswith("bin/") else 0o100644
            ) << 16
            archive.writestr(entry, content)
    with zipfile.ZipFile(zip_path) as archive:
        if set(archive.namelist()) != files.keys() or any(
            archive.read(n) != value for n, value in files.items()
        ):
            raise ValueError("archive did not round-trip")
    sha = digest(zip_path.read_bytes())
    (out / "candidate.json").write_bytes(files["candidate.json"])
    (out / "SHA256SUMS").write_bytes(f"{sha}  {zip_path.name}\n".encode())
    print(
        json.dumps(
            {"archive": zip_path.name, "sha256": sha, "commit": source["commit"]}
        )
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--development", action="store_true")
    args = parser.parse_args()
    build(args.manifest.resolve(), args.out.resolve(), args.development)
