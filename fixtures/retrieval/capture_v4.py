"""Capture original deterministic lexical and hybrid ranks in the isolated fixed v4 checkout."""

from datetime import UTC, datetime
import json
import math
from pathlib import Path
import sqlite3
import subprocess
import sys
from yuki.memory.fts import build_safe_lexical_query
from yuki.memory.models import (
    MemoryEntityTarget,
    MemoryFact,
    MemoryLexicalCandidate,
    MemoryQuery,
)
from yuki.memory.embedding.models import MemorySemanticCandidate
from yuki.memory.ranking import MemoryRanker, relevance_band
from yuki.memory.query import normalize_query_text

SHA = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
assert subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() == SHA
target = MemoryEntityTarget(
    role="current_person",
    scope_type="person",
    canonical_subject_person_id="owner",
    block_id="mine",
)
text = "桂花烏龍"
rows = [
    ("桂花烏龍", "飲茶習慣", "春天", "2024-01-01T00:00:00Z", [0.5, 0.5], 3),
    ("桂花烏龍", "另一批桂花烏龍", "春天", "2024-01-04T00:00:00Z", [0.5, 0.5], 5),
    ("內容", "桂花烏龍", "茶", "2024-02-01T00:00:00Z", [0.9, 0.2], 3),
    ("分類", "旅行期間的偏好", "桂花烏龍", None, [0.6, 0.8], 4),
    ("習慣", "很喜歡桂花的香氣", "茶", "2024-01-08T00:00:00Z", [1.0, 0.0], 3),
    ("其他", "圖書館開放時間", "時間", "2024-01-05T00:00:00Z", [0.0, 1.0], 3),
]
stamp = datetime(2024, 1, 1, tzinfo=UTC)
facts = [
    MemoryFact(
        id=i,
        scope_type="person",
        canonical_subject_person_id="owner",
        kind="fact",
        memory_key=k,
        category=cat,
        content=body,
        normalized_content=normalize_query_text(body),
        importance=imp,
        confidence=0.8,
        source_type="explicit",
        status="active",
        created_at=stamp,
        updated_at=stamp,
    )
    for i, (k, body, cat, date, vec, imp) in enumerate(rows, 1)
]
db = sqlite3.connect(":memory:")
db.execute(
    "CREATE VIRTUAL TABLE v USING fts5(content,memory_key,category,tokenize='trigram')"
)
db.executemany(
    "INSERT INTO v(rowid,content,memory_key,category) VALUES(?,?,?,?)",
    [(i, r[1], r[0], r[2]) for i, r in enumerate(rows, 1)]
    + [(i, "private 桂花烏龍", "outside", "foreign") for i in range(7, 507)],
)
query = MemoryQuery(
    text=text,
    normalized_text=text,
    mode="relevant",
    targets=(target,),
    candidate_limit=50,
    limit_per_target=20,
    query_term_limit=12,
)
safe = build_safe_lexical_query(text, term_limit=12)
ranks = {
    i: score
    for i, score in db.execute(
        "SELECT rowid,bm25(v,1.,4.,2.) FROM v WHERE v MATCH ? AND rowid<=6",
        (safe.fts_expression,),
    )
}
lex = [
    MemoryLexicalCandidate(
        fact_id=f.id,
        target=target,
        fts_rank=ranks[f.id],
        exact_match=text in {f.memory_key, f.normalized_content, f.category},
    )
    for f in facts
    if f.id in ranks
]
scores = sorted(
    [(i, vec[0] / math.hypot(*vec)) for i, (_, _, _, _, vec, _) in enumerate(rows, 1)],
    key=lambda t: (-t[1], t[0]),
)
sem = [
    MemorySemanticCandidate(
        fact_id=i, target=target, cosine_similarity=s, semantic_rank=n
    )
    for n, (i, s) in enumerate(scores, 1)
]
ranker = MemoryRanker()
hits = ranker.rank_global(
    ranker.rank_hybrid(
        facts=tuple(facts),
        lexical_candidates=tuple(lex),
        semantic_candidates=tuple(sem),
        target=target,
        normalized_query=text,
        lexical_weight=1.0,
        semantic_weight=1.0,
        rrf_k=60,
        limit=50,
    ),
    query,
)
Path(sys.argv[1]).write_text(
    json.dumps(
        {
            "source_commit": SHA,
            "query": text,
            "rows": rows,
            "expected": [
                {
                    "fact_id": h.fact.id,
                    "band": relevance_band(h),
                    "reason": h.selection_reason,
                    "lexical_rank": h.lexical_rank,
                    "semantic_rank": h.semantic_rank,
                    "fusion": h.fusion_score,
                }
                for h in hits
            ],
        },
        ensure_ascii=False,
        indent=2,
    )
    + "\n",
    encoding="utf-8",
)
