"""Run only inside the isolated v4 environment; save the actual calendar and tool declarations."""

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
import subprocess
import sys

BASE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"


def record(source):
    if (
        subprocess.check_output(
            ["git", "-C", source, "rev-parse", "HEAD"], text=True
        ).strip()
        != BASE
    ):
        raise ValueError("incorrect oracle commit")
    sys.path.insert(0, str(source / "src"))
    from pydantic import TypeAdapter
    from yuki.automation.models import Schedule
    from yuki.time.schedules import initial_run_at, schedule_after_completion
    from yuki.automation.tools import AutomationToolService

    zone = "America/New_York"
    rules = [
        ({"type": "after", "seconds": 7}, "2026-03-08T01:30:00-05:00"),
        ({"type": "interval", "seconds": 17}, "2026-03-08T01:30:00-05:00"),
        (
            {"type": "once", "local_datetime": "2026-03-08T09:00:00+05:30"},
            "2026-03-07T10:00:00Z",
        ),
        (
            {"type": "daily", "hour": 2, "minute": 30, "timezone": zone},
            "2026-03-08T05:00:00Z",
        ),
        (
            {
                "type": "weekly",
                "weekdays": [7],
                "hour": 1,
                "minute": 30,
                "timezone": zone,
            },
            "2026-11-01T00:00:00Z",
        ),
        (
            {"type": "daily", "hour": 1, "minute": 30, "timezone": zone},
            "2026-11-01T06:00:00Z",
        ),
        (
            {
                "type": "daily",
                "hour": 2,
                "minute": 15,
                "timezone": "Australia/Lord_Howe",
            },
            "2026-10-03T15:00:00Z",
        ),
    ]
    rows = []
    for raw, stamp in rules:
        rule = TypeAdapter(Schedule).validate_python(raw)
        now = datetime.fromisoformat(stamp).astimezone(timezone.utc)
        first = initial_run_at(rule, now)
        late = first.replace(day=min(first.day + 3, 28))
        then = schedule_after_completion(rule, first, late)
        rows.append(
            {
                "trigger": raw,
                "now": now.isoformat(),
                "due": first.isoformat(),
                "settled": late.isoformat(),
                "next": then.isoformat() if then else None,
            }
        )
    tools = {
        t.name: {"description": t.description, "parameters": t.parameters}
        for t in AutomationToolService(None).definitions()
        if t.name.startswith("automation_")
    }
    return {"source_commit": BASE, "clock": rows, "tools": tools}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("out", type=Path)
    args = parser.parse_args()
    args.out.write_text(
        json.dumps(record(args.source), ensure_ascii=False, indent=2) + "\n"
    )
