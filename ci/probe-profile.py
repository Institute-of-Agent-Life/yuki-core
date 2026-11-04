"""Offline profile evidence and an explicitly authorized, single-attempt synthetic probe."""

import argparse
import datetime as dt
import hashlib
import http.client
import json
import os
import uuid
from decimal import Decimal
from pathlib import Path
from urllib.parse import urlsplit

DATA = Path(__file__).resolve().parents[1] / "baseline/profile-evidence"
FIELDS = ("endpoint", "region", "model", "wire", "thinking", "task")
PROMPT = "Return a JSON object with a results array containing title and url for an IANA example domain."
WIRES = ("chat", "responses", "messages", "gemini")


def identity(profile):
    fields = {k: profile[k] for k in FIELDS}
    if any(not isinstance(v, str) or not v.strip() for v in fields.values()):
        raise ValueError("profile fields must be explicit")
    return hashlib.sha256(
        json.dumps(fields, sort_keys=True, separators=(",", ":")).encode()
    ).hexdigest()


def sources(items):
    return sorted(
        {
            url
            for url in items
            if isinstance(url, str)
            and urlsplit(url).scheme in {"https", "http"}
            and urlsplit(url).hostname
        }
    )


def classify(wire, response, task, status):
    usage = response.get("usageMetadata" if wire == "gemini" else "usage")
    result = {"state": "ignored", "sources": [], "usage": usage, "cost_usd": None}
    if status < 200 or status >= 300 or "error" in response:
        result["state"] = "rejected"
        return result
    if task == "structured_search":
        if wire != "chat":
            return {**result, "state": "unsupported_wire_task"}
        choices = response.get("choices", [])
        try:
            content = json.loads(choices[0]["message"]["content"])
            records = content["results"]
            if (
                not isinstance(records, list)
                or not records
                or any(
                    not isinstance(r, dict) or not isinstance(r.get("title"), str)
                    for r in records
                )
            ):
                raise ValueError("missing results")
            urls = sources(r["url"] for r in records)
            if (
                choices[0].get("finish_reason") != "stop"
                or not urls
                or any(r["url"] not in urls for r in records)
            ):
                raise ValueError("unconfirmed output")
            return {**result, "state": "structured_result", "sources": urls}
        except (ValueError, KeyError, IndexError, TypeError):
            return {**result, "state": "invalid_structured_result"}
    evidence, links, failed = False, [], False
    if wire == "responses":
        for item in response.get("output", []):
            if item.get("type") == "web_search_call" and item.get("id"):
                if item.get("status") == "completed":
                    evidence = True
                    links.extend(
                        s.get("url") for s in item.get("action", {}).get("sources", [])
                    )
                else:
                    failed = True
    elif wire == "messages":
        blocks = response.get("content", [])
        calls = {
            b["id"]
            for b in blocks
            if b.get("type") == "server_tool_use"
            and b.get("name") == "web_search"
            and b.get("id")
        }
        for b in blocks:
            if (
                b.get("type") == "web_search_tool_result"
                and b.get("tool_use_id") in calls
            ):
                items = b.get("content")
                if isinstance(items, list):
                    evidence = True
                    links.extend(
                        i.get("url")
                        for i in items
                        if i.get("type") == "web_search_result"
                    )
                else:
                    failed = True
    elif wire == "gemini":
        for candidate in response.get("candidates", []):
            parts = candidate.get("content", {}).get("parts", [])
            calls = {
                c["id"]
                for p in parts
                if (c := p.get("toolCall", {})).get("toolType") == "GOOGLE_SEARCH_WEB"
                and c.get("id")
            }
            grounded = candidate.get("groundingMetadata", {})
            paired = [
                p["toolResponse"]
                for p in parts
                if p.get("toolResponse", {}).get("id") in calls
            ]
            if (grounded.get("webSearchQueries") or paired) and candidate.get(
                "finishReason"
            ) == "STOP":
                evidence = True
                links.extend(
                    c.get("web", {}).get("uri")
                    for c in grounded.get("groundingChunks", [])
                )
                links.extend(tool.get("response", {}).get("source") for tool in paired)
    valid = sources(links)
    state = (
        "native_sources"
        if evidence and valid
        else "no_source"
        if evidence
        else "tool_failed"
        if failed
        else "ignored"
    )
    return {**result, "state": state, "sources": valid}


def request(profile):
    wire, task = profile["wire"], profile["task"]
    if (
        wire not in WIRES
        or task not in ("structured_search", "native_search")
        or profile["thinking"] != "default"
    ):
        raise ValueError(
            "no bounded fixture for the requested wire/task/thinking setting"
        )
    model = profile["model"]
    if task == "structured_search" and wire == "chat":
        return {
            "model": model,
            "max_tokens": 64,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "user", "content": PROMPT + ' Example: {"results":[]}'}
            ],
        }
    if task != "native_search":
        raise ValueError("structured probe currently requires chat")
    if wire == "responses":
        return {
            "model": model,
            "input": PROMPT,
            "max_output_tokens": 128,
            "max_tool_calls": 1,
            "store": False,
            "tools": [{"type": "web_search"}],
            "include": ["web_search_call.action.sources"],
        }
    if wire == "messages":
        return {
            "model": model,
            "max_tokens": 128,
            "messages": [{"role": "user", "content": PROMPT}],
            "tools": [
                {"type": "web_search_20250305", "name": "web_search", "max_uses": 1}
            ],
        }
    if wire == "gemini":
        return {
            "contents": [{"role": "user", "parts": [{"text": PROMPT}]}],
            "generationConfig": {"maxOutputTokens": 128},
            "tools": [{"googleSearch": {}}],
        }
    raise ValueError("no generic native search event in this Chat wire fixture")


def run_probe(
    profile, out, *, live=False, authorization=None, key=None, reservation_dir=None
):
    out.mkdir(parents=True, exist_ok=False)
    profile = {k: profile[k] for k in FIELDS}
    endpoint = urlsplit(profile["endpoint"])
    if (
        endpoint.username
        or endpoint.password
        or endpoint.query
        or endpoint.fragment
        or not endpoint.hostname
    ):
        raise ValueError("endpoint must be a credential-free URL")
    fingerprint = identity(profile)
    report = {
        "profile": profile,
        "profile_key": fingerprint,
        "state": "not_authorized",
        "attempts": [],
        "usage": None,
        "cost_usd": None,
    }
    path = out / "probe.json"

    def save():
        path.write_text(
            json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8"
        )
        return report

    if not live or not authorization or not key:
        return save()
    auth = json.loads(authorization.read_bytes())
    if (
        auth.get("authorized") is not True
        or auth.get("profile_key") != fingerprint
        or auth.get("task") != profile["task"]
    ):
        return save()
    limit = Decimal(str(auth["max_usd"]))
    cap = Decimal(str(auth["endpoint_hard_cap_usd"]))
    if not (
        cap.is_finite()
        and limit.is_finite()
        and 0 < cap <= limit
        and auth.get("endpoint_cap_receipt")
    ):
        return save()
    now = dt.datetime.now(dt.timezone.utc)
    if dt.datetime.fromisoformat(auth["expires_at"]) <= now:
        return save()
    local = endpoint.scheme == "http" and endpoint.hostname in {
        "127.0.0.1",
        "::1",
        "localhost",
    }
    if endpoint.scheme != "https" and not local:
        raise ValueError("remote probes require HTTPS")
    body = request(profile)
    if profile["wire"] == "gemini" and not endpoint.path.endswith(
        "/models/" + profile["model"] + ":generateContent"
    ):
        raise ValueError("Gemini endpoint does not select the authorized model")
    # One opt-in authorizes one physical attempt, including uncertain transport failures.
    authorization_id = str(uuid.UUID(auth["authorization_id"]))
    ledger = (
        reservation_dir or Path.home() / ".local/state/yuki-core/probe-authorizations"
    )
    ledger.mkdir(parents=True, exist_ok=True, mode=0o700)
    try:
        with (ledger / (authorization_id + ".json")).open(
            "x", encoding="utf-8"
        ) as reservation:
            json.dump(
                {
                    "profile_key": fingerprint,
                    "reserved_usd": str(limit),
                    "created": now.isoformat(),
                },
                reservation,
            )
            reservation.flush()
            os.fsync(reservation.fileno())
    except FileExistsError:
        report["state"] = "already_reserved"
        return save()
    connection_type = (
        http.client.HTTPConnection if local else http.client.HTTPSConnection
    )
    conn = connection_type(endpoint.hostname, endpoint.port, timeout=15)
    headers = {"Content-Type": "application/json"}
    if profile["wire"] == "messages":
        headers.update({"x-api-key": key, "anthropic-version": "2023-06-01"})
    elif profile["wire"] == "gemini":
        headers["x-goog-api-key"] = key
    else:
        headers["Authorization"] = "Bearer " + key
    attempt = {
        "ordinal": 1,
        "state": "dispatching",
        "request": body,
        "response": None,
        "status": None,
    }
    report.update(state="unknown", attempts=[attempt])
    save()
    try:
        conn.request("POST", endpoint.path or "/", json.dumps(body).encode(), headers)
        response = conn.getresponse()
        attempt["status"] = response.status
        raw = response.read(1048577)
        if len(raw) > 1048576:
            raise ValueError("response exceeds capture limit")
        # The probe has synthetic input only. Never persist a reflected credential.
        payload = json.loads(raw.decode().replace(key, "[redacted]"))
        if not isinstance(payload, dict):
            raise ValueError("response body is not an object")
        attempt.update(state="response", response=payload)
        report.update(
            classify(profile["wire"], payload, profile["task"], response.status)
        )
    except (
        OSError,
        http.client.HTTPException,
        ValueError,
        TypeError,
        KeyError,
    ) as error:
        attempt.update(state="unknown", error_kind=type(error).__name__)
    finally:
        conn.close()
    return save()


def grid():
    docs = json.loads((DATA / "sources.json").read_bytes())
    raw = (DATA / "wire.json").read_bytes()
    if hashlib.sha256(raw).hexdigest() != docs["wire_sha256"]:
        raise ValueError("v4 wire digest changed")
    source = json.loads(raw)
    records = []
    for wire in WIRES:
        case = source["cases"][wire]
        first = case["attempts"][0]
        expected_path = {
            "chat": "/chat/completions",
            "responses": "/responses",
            "messages": "/messages",
            "gemini": ":generateContent",
        }[wire]
        assert first["method"] == "POST" and first["path"].endswith(expected_path)
        records.append(
            {
                "wire": wire,
                "synthetic_profile": {
                    "endpoint": "https://wire.invalid" + first["path"],
                    "model": first["request"].get("model", "synthetic"),
                    "thinking": "see captured request",
                    "region": "fixture",
                    "task": "native_search",
                },
                "baseline": {
                    "source_commit": source["source_commit"],
                    "nodeid": case["nodeid"],
                    "case_sha256": case["case_sha256"],
                    "junit_sha256": source["junit_sha256"],
                },
                "official_document": {
                    **docs["documents"][wire],
                    "reviewed": docs["reviewed"],
                },
                "offline_replay": classify(
                    wire, first["response"], "native_search", first["status"]
                ),
                "real_endpoint": {
                    "state": "not_authorized",
                    "usage": None,
                    "cost_usd": None,
                },
            }
        )
    return {
        "original_bridge_cases": {
            k: source["cases"][k]["nodeid"]
            for k in ("cache_isolated", "missing_sources")
        },
        "cases": {"H23": records},
        "wire_sha256": docs["wire_sha256"],
        "adapter_sources": source["adapter_files"],
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--profile", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--live", action="store_true")
    parser.add_argument("--authorization", type=Path)
    parser.add_argument("--key-env")
    args = parser.parse_args()
    if args.profile:
        run_probe(
            json.loads(args.profile.read_bytes()),
            args.out,
            live=args.live,
            authorization=args.authorization,
            key=os.environ.get(args.key_env) if args.key_env else None,
        )
    else:
        args.out.mkdir(parents=True, exist_ok=False)
        (args.out / "endpoint-evidence-grid.json").write_text(
            json.dumps(grid(), indent=2), encoding="utf-8"
        )
