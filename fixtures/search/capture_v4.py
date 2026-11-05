"""Run against the pinned isolated bridge. A synthetic provider exposes the original request."""
import asyncio
import json
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

from tests.unit.test_native_search_bridge import _bridge
from yuki.web.models import WebSearchRequest


async def main():
    source = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    assert source == "329ca7c487414d2f54ddd98ff4727446ed2446cd"
    request = WebSearchRequest("茶具", max_results=2)
    with TemporaryDirectory() as temp:
        bridge, fake = _bridge(Path(temp))
        original = await bridge.search(request)
        again = await bridge.search(request)
        sent = fake.requests[0]
        assert len(fake.requests) == 1
        bounded=WebSearchRequest("茶具", topic="news", time_range="week", max_results=2)
        constrained=await bridge.search(bounded);await bridge.search(bounded)
        return {"constrained_prompt":fake.requests[1].messages[0].content,"dated_partial":constrained.partial_failure,"all_physical_calls":len(fake.requests),"source_commit":source, "user":sent.messages[0].content, "local_functions":len(sent.tools),
                "complete_physical_calls":1,"original_urls":[s.url for s in original.sources],
                "cache_urls":[s.url for s in again.sources],"partial":again.partial_failure}

if __name__ == "__main__":
    Path(sys.argv[1]).write_text(json.dumps(asyncio.run(main()),ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
