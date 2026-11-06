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


async def test_original_group_attribution(database):
    from tests.unit import test_memory_mutation as old
    from yuki.memory.mutation.models import MemoryMutationRequest

    service, facts, ledger, _ = old._service(database)

    async def event(tag, text, mentions=()):
        return await old._event(
            ledger,
            message_id=tag,
            sender_user_id="1001",
            content=text,
            group_id="3001",
            mentioned_user_ids=mentions,
        )

    async def apply(e, op, reference, **fields):
        req = MemoryMutationRequest(
            operation=op,
            target={"scope_type": "person_group", "subject_ref": reference},
            reason="misattributed_subject",
            **fields,
        )
        return await service.mutate(req, old._context(e))

    async def snapshot(fid):
        f = await facts.get_fact(fid)
        es = await facts.list_evidence(fid)
        return {
            "subject": f.subject_user_id,
            "group": f.group_id,
            "status": f.status.value,
            "scope": f.scope_type.value,
            "content": f.content,
            "authority": f.authority.value,
            "evidence": sorted(
                [
                    {
                        "author": e.source_speaker_user_id,
                        "quote": e.excerpt,
                        "authority": e.authority.value,
                    }
                    for e in es
                ],
                key=lambda x: x["quote"],
            ),
        }

    e1 = await event("own", "我喜欢摄影")
    origin = await apply(
        e1,
        "create",
        "current_speaker",
        new_content="喜欢摄影",
        memory_key="hobby:photography",
        category="hobby",
    )
    assert origin.ok
    initial = await snapshot(origin.new_fact_id)
    e2 = await event("attribution", "刚才那条其实说的是小明喜欢摄影", ("2002",))
    moved = await apply(e2, "reassign", "mentioned_user", fact_id=origin.new_fact_id)
    assert (
        moved.ok
        and moved.old_fact_id == origin.new_fact_id
        and moved.new_fact_id != origin.new_fact_id
    )
    e3 = await event("two", "两位都提到了", ("2002", "2003"))
    rejected = await apply(
        e3,
        "create",
        "mentioned_user",
        new_content="有歧义",
        memory_key="invalid",
        category="identity",
    )
    assert not rejected.ok
    e4 = await event("third", "江环是@鬼頭桃菜，你记错了", ("2002",))
    third = await apply(
        e4,
        "create",
        "mentioned_user",
        new_content="江环是鬼頭桃菜",
        memory_key="identity:jianghuan",
        category="identity",
        evidence_quote="江环是@鬼頭桃菜，你记错了",
    )
    assert third.ok
    d = {
        "created": initial,
        "superseded": await snapshot(origin.new_fact_id),
        "reassigned": await snapshot(moved.new_fact_id),
        "direct": await snapshot(third.new_fact_id),
        "distinct_version": True,
        "ambiguous_reason": rejected.reason_code,
        "reassign_reason": moved.reason_code,
        "reassign_outcome": moved.outcome.value,
    }
    (Path(os.environ["YUKI_ATTRIBUTION_ORACLE_OUT"]) / "original.json").write_text(
        json.dumps(d, indent=2, ensure_ascii=False) + "\n"
    )


async def test_original_reassign(database):
    from tests.unit.test_memory_mutation import (
        test_reassign_is_one_atomic_versioned_group_operation,
    )

    await test_reassign_is_one_atomic_versioned_group_operation(database)


async def test_original_word_order(database):
    from tests.unit.test_memory_mutation import (
        test_mentioned_subject_is_not_rejected_by_chinese_word_order,
    )

    for sentence in [
        "江环是@鬼頭桃菜，你记错了",
        "@鬼頭桃菜是江环，这次请按这个主体纠正",
    ]:
        await test_mentioned_subject_is_not_rejected_by_chinese_word_order(
            database, sentence
        )


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
    os.environ["YUKI_ATTRIBUTION_ORACLE_OUT"] = str(out)
    os.chdir(source)
    sys.path[:0] = [str(source), str(source / "src"), str(core)]
    from baseline.replay.record import offline
    import pytest

    offline()
    with tempfile.TemporaryDirectory(prefix="yuki-original-attribution-") as tmp:
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
        "src/yuki/memory/service.py",
        "src/yuki/memory/mutation/service.py",
        "src/yuki/memory/models.py",
        "tests/unit/test_memory_mutation.py",
    ]
    audit = {
        "source_commit": BASE,
        "source_files": {
            n: hashlib.sha256((source / n).read_bytes()).hexdigest() for n in files
        },
        "junit_sha256": hashlib.sha256((out / "junit.xml").read_bytes()).hexdigest(),
        "projection": json.loads((out / "original.json").read_bytes()),
    }
    (out / "v4-group-attribution.json").write_text(
        json.dumps(audit, indent=2, ensure_ascii=False) + "\n"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
