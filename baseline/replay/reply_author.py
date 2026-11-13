"""Run this file via the isolated pinned v4 interpreter, never against a live Yuki database."""

import argparse
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import tempfile

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
pytest_plugins = ["tests.conftest"]


async def test_original_reply_author(database):
    from tests.unit import test_memory_mutation as old
    from tests.support.event_seed import seed_event
    from yuki.domain.conversations import ScopeType
    from yuki.memory.mutation.models import MemoryMutationRequest

    service, facts, ledger, _ = old._service(database)
    async def event(tag, author, reply=None):
        result, _ = await seed_event(ledger, bot_user_id="8000", scope_type=ScopeType.GROUP,
            sender_user_id=author, group_id="3001", origin_key=tag, direction="inbound",
            content="他喜欢摄影" if author == "1001" else "大家好",
            reply_to_event_id=reply)
        return result
    async def apply(e, op="create", reference="replied_message_author", **fields):
        return await service.mutate(MemoryMutationRequest(operation=op,
            target={"scope_type":"person_group", "subject_ref":reference},
            reason="misattributed_subject", **fields), old._context(e))
    async def snapshot(result):
        output={"ok":result.ok,"reason":result.reason_code}
        if result.ok:
            f=await facts.get_fact(result.new_fact_id)
            output.update(subject=f.subject_user_id, scope=f.scope_type.value, group=f.group_id,
                authority=f.authority.value, content=f.content,
                evidence=[{"author":e.source_speaker_user_id,"quote":e.excerpt,"authority":e.authority.value}
                    for e in await facts.list_evidence(f.id)])
        return output
    other=await event("reply-target","2002")
    current=await event("reply-trigger","1001",other.id)
    fields=dict(new_content="喜欢摄影",memory_key="reply:photography",category="hobby")
    direct=await apply(current,**fields)
    own=await event("own-source","1001")
    self_reply=await event("self-reply","1001",own.id)
    self_result=await apply(self_reply,**dict(fields,memory_key="reply:self"))
    origin=await apply(own,reference="current_speaker",**dict(fields,memory_key="reply:origin"))
    moved=await apply(current,op="reassign",fact_id=origin.new_fact_id)
    self_origin=await apply(own,reference="current_speaker",**dict(fields,memory_key="reply:self-origin"))
    self_moved=await apply(self_reply,op="reassign",fact_id=self_origin.new_fact_id)
    missing=await apply(own,**dict(fields,memory_key="reply:missing"))
    result={"direct":await snapshot(direct),"self_reply":await snapshot(self_result),
        "reassigned":await snapshot(moved),"self_reassigned":await snapshot(self_moved),"missing":await snapshot(missing)}
    assert direct.ok and self_result.ok and moved.ok and not self_moved.ok and not missing.ok
    (Path(os.environ["YUKI_REPLY_AUTHOR_ORACLE_OUT"])/"original.json").write_text(
        json.dumps(result,indent=2,ensure_ascii=False)+"\n")


async def test_reply_identity_is_frozen_at_admission(database):
    from tests.unit.test_canonical_ingress import _Bot, _message, _stack
    from tests.support.canonical_identity import ensure_presence, ensure_space
    from yuki.persistence.models import ChatEventModel

    registry, resolver, writer = await _stack(database)
    bot = _Bot("8000")
    async with database.immediate_session() as session:
        presence = await ensure_presence(session, bot.self_id)
        await ensure_space(session, "2001")
    registry.connect(bot, provider_id="snowluma", presence_id=presence)
    current = await resolver.pre_admit(bot, _message(message_id="reply-before-target", group_id="2001", reply_to_message_id="later-target"))
    assert current.message.reply_to_event_id is None
    stored = await writer.append_inbound(current.message, current)
    later = await resolver.pre_admit(bot, _message(message_id="later-target", group_id="2001", user_id="2002"))
    target = await writer.append_inbound(later.message, later)
    assert target.event.id > stored.event.id
    async with database.read_session() as session:
        assert (await session.get(ChatEventModel, stored.event.id)).reply_to_event_id is None
    known = await resolver.pre_admit(bot, _message(message_id="reply-after-target", group_id="2001", reply_to_message_id="later-target"))
    assert known.message.reply_to_event_id == target.event.id
    (Path(os.environ["YUKI_REPLY_AUTHOR_ORACLE_OUT"])/"admission.json").write_text(json.dumps({
        "unresolved_original_remains_unresolved":True,"later_reply_uses_existing_original":True,
        "source_event":stored.event.id,"later_target":target.event.id})+"\n")


def main():
    cli = argparse.ArgumentParser()
    cli.add_argument("--source", type=Path, required=True)
    cli.add_argument("--out", type=Path, required=True)
    args = cli.parse_args()
    source = args.source.resolve(strict=True)
    out = args.out.resolve()
    here = Path(__file__).resolve()
    core = here.parents[2]
    assert (
        subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source, text=True
        ).strip()
        == BASE
    )
    assert not subprocess.check_output(["git", "diff", "HEAD", "--"], cwd=source)
    out.mkdir(parents=True, exist_ok=False)
    os.environ["YUKI_REPLY_AUTHOR_ORACLE_OUT"] = str(out)
    os.chdir(source)
    sys.path[:0] = [str(source), str(source / "src"), str(core)]
    from baseline.replay.record import offline
    import pytest

    offline()
    with tempfile.TemporaryDirectory(prefix="yuki-original-reply-author-") as tmp:
        code = pytest.main(
            [
                "-q",
                "-p",
                "no:cacheprovider",
                "-c",
                str(source / "pyproject.toml"),
                "--basetemp",
                tmp,
                "--junitxml",
                str(out / "junit.xml"),
                str(here),
            ]
        )
    if code:
        return int(code)
    files = [
        "src/yuki/memory/subjects.py",
        "src/yuki/identity/ingress.py",
        "src/yuki/memory/mutation/service.py",
        "src/yuki/memory/models.py",
        "tests/unit/test_memory_mutation.py",
        "tests/unit/test_canonical_ingress.py",
    ]
    audit = {
        "source_commit": BASE,
        "source_files": {
            n: hashlib.sha256((source / n).read_bytes()).hexdigest() for n in files
        },
        "junit_sha256": hashlib.sha256((out / "junit.xml").read_bytes()).hexdigest(),
        "projection": json.loads((out / "original.json").read_bytes()),
        "admission": json.loads((out / "admission.json").read_bytes()),
    }
    (out / "v4-reply-author.json").write_text(
        json.dumps(audit, indent=2, ensure_ascii=False) + "\n"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
