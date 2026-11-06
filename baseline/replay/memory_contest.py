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


async def test_original_contest_pair(database):
    from sqlalchemy import select
    from tests.unit import test_memory_mutation as old
    from yuki.persistence.models import MemoryFactRelationModel
    from yuki.memory.mutation.models import MemoryMutationRequest
    service, facts, ledger, _ = old._service(database)
    target = {"subject_ref":"current_speaker","scope_type":"person"}
    async def action(tag,text,operation,**fields):
        event=await old._event(ledger,message_id=tag,sender_user_id="1001",content=text)
        r=await service.mutate(MemoryMutationRequest(operation=operation,target=target,reason="user_requested_review",**fields),old._context(event))
        assert r.ok,r
        return r
    created=await action("original","我喜欢爵士乐","create",new_content="喜欢爵士乐",memory_key="music:favorite",category="music")
    fid=created.new_fact_id
    async def snapshot(i):
        fact=await facts.get_fact(i)
        return {"status":fact.status.value,"conflict":fact.conflict_state.value,"content":fact.content,"evidence_count":len(await facts.list_evidence(i))}
    before=await snapshot(fid)
    mark=await action("mark","先标为有争议","contest",fact_id=fid)
    same=await snapshot(fid)
    alt=await action("alternative","我喜欢摇滚乐","contest",fact_id=fid,new_content="喜欢摇滚乐",memory_key="music:favorite",category="music")
    assert alt.new_fact_id!=fid and mark.new_fact_id==fid
    async with database.read_session() as session:
        links=(await session.scalars(select(MemoryFactRelationModel))).all()
    result={"before":before,"marked":same,"after_alternative":await snapshot(fid),"alternative":await snapshot(alt.new_fact_id),
            "mark_outcome":mark.outcome.value,"alternative_outcome":alt.outcome.value,
            "links":[{"type":r.relation_type,"source_is_alternative":r.source_fact_id==alt.new_fact_id,"target_is_original":r.target_fact_id==fid} for r in links]}
    (Path(os.environ["YUKI_CONTEST_ORACLE_OUT"])/"original.json").write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")


async def test_original_transition_suite(database):
    from tests.unit.test_memory_mutation import test_merge_metadata_contest_invalidate_and_restore_operations
    await test_merge_metadata_contest_invalidate_and_restore_operations(database)

async def test_original_embedding_after_commit(database):
    from tests.unit.test_memory_mutation import test_contested_claim_is_enqueued_after_commit
    await test_contested_claim_is_enqueued_after_commit(database)

def main():
    cli=argparse.ArgumentParser();cli.add_argument("--source",type=Path,required=True);cli.add_argument("--out",type=Path,required=True)
    args=cli.parse_args();source=args.source.resolve(strict=True);out=args.out.resolve();here=Path(__file__).resolve();core=here.parents[2]
    assert subprocess.check_output(["git","rev-parse","HEAD"],cwd=source,text=True).strip()==BASE
    assert not subprocess.check_output(["git","diff","HEAD","--"],cwd=source)
    out.mkdir(parents=True,exist_ok=False);os.environ["YUKI_CONTEST_ORACLE_OUT"]=str(out);os.chdir(source);sys.path[:0]=[str(source),str(source/"src"),str(core)]
    from baseline.replay.record import offline
    import pytest
    offline()
    with tempfile.TemporaryDirectory(prefix="yuki-original-contest-") as tmp:
        code=pytest.main(["-q","-p","no:cacheprovider","-c",str(source/"pyproject.toml"),"--basetemp",tmp,"--junitxml",str(out/"junit.xml"),str(here)])
    if code:return int(code)
    files=["src/yuki/memory/service.py","src/yuki/memory/mutation/service.py","src/yuki/memory/models.py","tests/unit/test_memory_mutation.py"]
    audit={"source_commit":BASE,"source_files":{n:hashlib.sha256((source/n).read_bytes()).hexdigest() for n in files},"junit_sha256":hashlib.sha256((out/"junit.xml").read_bytes()).hexdigest(),"projection":json.loads((out/"original.json").read_bytes())}
    (out/"v4-contest.json").write_text(json.dumps(audit,indent=2,ensure_ascii=False)+"\n")
    return 0

if __name__=="__main__":
    sys.exit(main())
