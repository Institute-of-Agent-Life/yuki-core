"""Four real immutable consumer/Host combinations, including isolated negative peers."""

import copy
import sys
import tempfile
from pathlib import Path

from tests.peers.base.consumer_matrix import replay


def run(ctx):
    sys.path.insert(0, str(ctx.root / "ci"))
    from compat_matrix import (
        asset_path,
        check_runtime,
        original_assets,
        sha,
        verify_asset,
    )
    from source import source_identity

    lock, selected, stable_host, consumers = original_assets(ctx.root, ctx.working)
    changed = copy.deepcopy(selected["stable"])
    changed["sha256"] = "0" * 64
    ctx.rejects(
        "changed published bytes are rejected",
        ValueError,
        lambda: verify_asset(asset_path(ctx.root, selected["stable"]), changed),
    )
    candidate = (
        ctx.root
        / "target/release"
        / ("yuki-core.exe" if sys.platform == "win32" else "yuki-core")
    )
    current = source_identity(ctx.root)
    cells = {}
    for host_version, host in [("stable", stable_host), ("candidate", candidate)]:
        for consumer_version, consumer in consumers.items():
            label = host_version + "-host_" + consumer_version + "-plugin"
            results = {}
            for scenario in ("confirmed", "lost-second"):
                with tempfile.TemporaryDirectory(prefix="yuki-compat-") as temporary:
                    result = replay(
                        host,
                        consumer["binary"],
                        ctx.output / (label + "_" + scenario),
                        Path(temporary),
                        scenario,
                    )
                ctx.equal(
                    label + " " + scenario + " preserves the original effect",
                    result["status"],
                    "PASS",
                )
                ctx.equal(
                    label + " " + scenario + " has zero restart effects",
                    result["post_restart_effects"],
                    0,
                )
                ctx.check(
                    label + " rejects only incompatible installations",
                    len(result["isolated_rejections"]) == 2,
                )
                if host_version == "candidate":
                    ctx.write("sqlite-build.json", result["database_info"])
                results[scenario] = result
            cells[label] = {
                "status": "supported",
                "wire_profile": "plugin-base-1.0",
                "host": {
                    "source": (
                        lock["base_commit"]
                        if host_version == "stable"
                        else current["commit"]
                    ),
                    "sha256": sha(host),
                },
                "consumer": {
                    "source": lock[consumer_version + "_commit"],
                    "version": lock[consumer_version + "_version"],
                    "sdk": lock[consumer_version + "_sdk"],
                    "asset": consumer["asset"],
                    "binary_sha256": sha(consumer["binary"]),
                },
                "cases": results,
            }
    return {
        "status": "PASS",
        "previous_stable": lock["previous_stable"],
        "cells": cells,
        "minimum_protocol_version": lock["minimum_protocol_version"],
        "lock_sha256": sha(ctx.root / "ci/compat-lock.json"),
        "runtime": check_runtime(lock),
        "published_stable_consumer": lock["stable_commit"],
        "candidate_published": False,
        "paid_calls": 0,
    }
