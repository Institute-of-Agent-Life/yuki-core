"""Run selected original v4 adapter cases and capture synthetic HTTP bodies without headers."""

import argparse
import hashlib
import json
import os
import sys
import subprocess
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
BASELINE = "329ca7c487414d2f54ddd98ff4727446ed2446cd"
TEST = "tests/unit/test_provider_native_result_boundaries.py::test_native_only_complete_keeps_real_checkpoint_usage_and_http_shape"
CASES = {
    "chat": "tests/unit/test_provider_protocols.py::test_chat_reasoning_citations_and_usage[stop]",
    "responses": "tests/unit/test_native_search_provenance.py::test_responses_sources_include_and_original_tail_survive_replay[OpenAIResponsesProvider]",
    "messages": TEST + "[AnthropicMessagesProvider]",
    "gemini": TEST + "[GeminiProvider]",
    "cache_isolated": "tests/unit/test_native_search_bridge.py::test_cache_isolates_actual_connection_and_request_profile[protocol]",
    "missing_sources": "tests/unit/test_native_search_bridge.py::test_failure_never_searches_extraction_backend_or_caches_failure[body_only]",
    "chat_structured": "tests/unit/test_provider_protocols.py::test_image_and_json_schema_reach_wire[OpenAICompatibleProvider]",
}


def main():
    import pytest
    import httpx

    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    source, out = args.source.resolve(), args.out.resolve()
    assert (
        subprocess.check_output(["git", "-C", source, "rev-parse", "HEAD"])
        .decode()
        .strip()
        == BASELINE
    )
    subprocess.run(["git", "-C", source, "diff", "--quiet", "HEAD", "--"], check=True)
    out.mkdir(parents=True, exist_ok=False)
    os.chdir(source)
    sys.path[:0] = [str(source), str(source / "src"), str(ROOT / "tests/harness")]
    import network

    network.install()
    from yuki.llm.openai_compatible import OpenAICompatibleProvider

    records = {}

    class Plugin:
        def pytest_runtest_setup(self, item):
            self.name = next(k for k, v in CASES.items() if v == item.nodeid)
            file = source / item.nodeid.split("::")[0]
            records[self.name] = {
                "nodeid": item.nodeid,
                "case_sha256": hashlib.sha256(file.read_bytes()).hexdigest(),
                "attempts": [],
                "payloads": [],
            }

    plugin = Plugin()
    send = httpx.MockTransport.handle_async_request

    async def capture(transport, request):
        response = await send(transport, request)
        records[plugin.name]["attempts"].append(
            {
                "method": request.method,
                "path": request.url.path,
                "request": json.loads(await request.aread()),
                "response": json.loads(await response.aread()),
                "status": response.status_code,
            }
        )
        return response

    build = OpenAICompatibleProvider._build_payload

    def payload(adapter, request):
        value = build(adapter, request)
        if plugin.name == "chat_structured":
            records[plugin.name]["payloads"].append(value)
        return value

    with (
        patch.object(httpx.MockTransport, "handle_async_request", capture),
        patch.object(OpenAICompatibleProvider, "_build_payload", payload),
    ):
        code = pytest.main(
            [
                "-q",
                "-c",
                str(source / "pyproject.toml"),
                "--rootdir",
                str(source),
                "--junitxml",
                str(out / "junit.xml"),
                *CASES.values(),
            ],
            plugins=[plugin],
        )
    assert code == 0
    for key, entry in records.items():
        if key == "chat_structured":
            assert entry["payloads"]
        elif key not in ("cache_isolated", "missing_sources"):
            assert entry["attempts"]
    adapters = ["openai_compatible", "openai_responses", "anthropic_messages", "gemini"]
    sources = {
        f"src/yuki/llm/{name}.py": hashlib.sha256(
            (source / f"src/yuki/llm/{name}.py").read_bytes()
        ).hexdigest()
        for name in adapters
    }
    data = {
        "source_commit": BASELINE,
        "junit_sha256": hashlib.sha256((out / "junit.xml").read_bytes()).hexdigest(),
        "adapter_files": sources,
        "cases": records,
    }
    (out / "wire.json").write_bytes(
        (json.dumps(data, sort_keys=True, indent=2) + "\n").encode()
    )


if __name__ == "__main__":
    main()
