import hashlib, json, subprocess, sys
from pathlib import Path

legacy = Path(
    "/Users/huangjiawei/.cache/yuki-core/329ca7c487414d2f54ddd98ff4727446ed2446cd"
)
sys.path.insert(0, str(legacy / "src"))
from yuki.social.content import NameTarget, _positions, _name_matches, _render_content
from yuki.domain.messages import sanitize_display_name

baseline = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
assert (
    subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=legacy, text=True).strip()
    == baseline
)
assert subprocess.run(["git", "diff", "--quiet"], cwd=legacy).returncode == 0
account = "00000000-0000-4000-8000-000000000001"
other = "00000000-0000-4000-8000-000000000002"
target = NameTarget(account, "ou_original", "小雪", "stage-original")
second = NameTarget(other, "ou_second", "小雪子", "stage-second")
names = [("小雪", target), ("小雪子", second)]
cases = []
for raw in [
    "你好 @小雪，@小雪子",
    r"跳脫 \@小雪，https://example.test/@小雪 alice@example.test @\u3000小雪".replace(
        r"\u3000", "\u3000"
    ),
    "@{account:" + account + "} / @{account:00000000000040008000000000000001}",
    "@未知 / @{account:00000000-0000-4000-8000-000000000009}",
    "@\n小雪 @\t小雪",
    "名 @小雪子 @小雪",
]:
    pos = _positions(raw)
    result = _render_content(
        raw, pos, _name_matches(raw, pos, names), {account: target}, None, {}
    )
    cases.append(
        {
            "input": raw,
            "text": result.message.text,
            "mentions": [
                {
                    "start": len(result.message.text[: m.text_start].encode()),
                    "end": len(result.message.text[: m.text_end].encode()),
                    "account": m.account_id,
                    "native": m.external_id,
                    "label": m.display_name,
                    "assignment": m.assignment_id,
                    "source": 1 if m.account_id == account else 2,
                }
                for m in result.message.mentions
            ],
        }
    )
result = {
    "baseline": baseline,
    "source_sha256": hashlib.sha256(
        (legacy / "src/yuki/social/content.py").read_bytes()
    ).hexdigest(),
    "cases": cases,
    "sanitized_names": [
        {"input": raw, "result": sanitize_display_name(raw)}
        for raw in ["  小\u200b雪  \t人\n", "雪" * 140, "a\u2028b"]
    ],
}
Path(sys.argv[1]).write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n")
assert subprocess.run(["git", "diff", "--quiet"], cwd=legacy).returncode == 0
