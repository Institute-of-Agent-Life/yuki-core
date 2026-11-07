"""An installed SDK consumer validates original annotation pages and physical ranges."""

import base64
import hashlib
import json
import time


def read(call, note, folder, mode, handle, descriptor, method):
    assert "error" in call(method, {"handle": "foreign", "offset": 0, "length": 1})
    data = bytearray()
    while len(data) < descriptor["bytes"]:
        page = call(method, {"handle": handle, "offset": len(data), "length": 12288})[
            "result"
        ]
        assert page["offset"] == len(data) and page["total"] == descriptor["bytes"]
        assert page["sha256"] == descriptor["sha256"]
        chunk = base64.b64decode(page["bytes"], validate=True)
        assert chunk and len(chunk) <= 12288
        data.extend(chunk)
        assert page["done"] == (len(data) == descriptor["bytes"])
        if (
            len(data) == len(chunk)
            and method == "gateway.read_mentions"
            and mode in {"mention-authority", "mention-erased"}
        ):
            note("reading-" + mode, {"handle": handle, "offset": len(data)})
            deadline = time.monotonic() + 30
            while not (folder / ("resume-" + mode)).exists():
                if time.monotonic() > deadline:
                    raise TimeoutError("original annotation authority barrier")
                time.sleep(0.01)
            # Recheck even when this annotation fit in the first page.
            assert "error" not in call(
                method, {"handle": handle, "offset": 0, "length": 1}
            )
    assert (
        len(data) == descriptor["bytes"]
        and hashlib.sha256(data).hexdigest() == descriptor["sha256"]
    )
    return bytes(data)


def serve(call, note, folder, mode, method, params):
    args = params["arguments"]
    handle = params["handle"]
    annotation_source = args.get("mentions_source")
    annotations = (
        json.loads(
            read(
                call,
                note,
                folder,
                mode,
                handle,
                annotation_source,
                "gateway.read_mentions",
            )
        )
        if annotation_source
        else []
    )
    assert args.get("mentions") is None
    if method == "gateway.plan_send":
        text_source = args.get("text_source")
        text = (
            read(
                call, note, folder, mode, handle, text_source, "gateway.read_text"
            ).decode()
            if text_source
            else args["message"]["text"]
        )
        raw = text.encode()
        note(
            "planned-" + mode, {"text": text, "mentions": annotations, "handle": handle}
        )
        parts = []
        start = 0
        while start < len(raw):
            end = min(start + 10000, len(raw))
            while end < len(raw) and raw[end] & 0xC0 == 0x80:
                end -= 1
            for mention in annotations:
                if mention["start"] < end < mention["end"]:
                    end = (
                        mention["start"] if mention["start"] > start else mention["end"]
                    )
            parts.append(
                {
                    "text_range": {
                        "start": start,
                        "length": end - start,
                        "sha256": hashlib.sha256(raw[start:end]).hexdigest(),
                    }
                }
            )
            start = end
        if mode == "mention-cut":
            cut = annotations[0]["start"] + 1
            parts = [
                {
                    "text_range": {
                        "start": 0,
                        "length": cut,
                        "sha256": hashlib.sha256(raw[:cut]).hexdigest(),
                    }
                },
                {
                    "text_range": {
                        "start": cut,
                        "length": len(raw) - cut,
                        "sha256": hashlib.sha256(raw[cut:]).hexdigest(),
                    }
                },
            ]
        if mode == "mention-replaced":
            parts[0]["mentions"] = [{**annotations[0], "native": "foreign-open-id"}]
        if mode == "mention-gap":
            parts[-1]["text_range"]["start"] += 1
        if mode == "mention-stage":
            note("reading-" + mode, {"handle": handle})
            deadline = time.monotonic() + 30
            while not (folder / ("resume-" + mode)).exists():
                if time.monotonic() > deadline:
                    raise TimeoutError("original stage barrier")
                time.sleep(0.01)
        if "reply" in args:
            parts[0]["reply"] = args["reply"]
        return {"parts": parts}
    selected = args["part"]["text_range"]
    descriptor = {"bytes": selected["length"], "sha256": selected["sha256"]}
    text = read(
        call, note, folder, mode, handle, descriptor, "gateway.read_text"
    ).decode()
    for mention in annotations:
        assert text.encode()[mention["start"] : mention["end"]].decode().startswith("@")
    index = args["part_ordinal"]
    note(
        "sent-" + mode + "-" + str(index),
        {"index": index, "text": text, "mentions": annotations, "route": args["route"]},
    )
    return (
        {"state": "unknown"}
        if mode == "mention-unknown" and index == 1
        else {"state": "confirmed", "locator": mode + "-" + str(index)}
    )
