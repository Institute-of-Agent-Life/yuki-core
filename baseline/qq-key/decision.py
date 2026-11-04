"""Bind a QQ duplicate-key decision to observed packet pairs and one deployment image."""
import argparse
import hashlib
import json
import re
from collections import defaultdict
from pathlib import Path


def assess(batch, *, originals_verified=False):
    image = batch.get("target_image_digest")
    actual = batch["kind"] == "target_capture" and originals_verified
    evidence = {"target_image_digest":image, "capture_image_digest":batch.get("capture_image_digest"),
                "kind":batch["kind"], "deployment_verified":False,
                "private":{"mode":"per_receiving_bot","cross_bot_shared_key":False},
                "group":{"mode":"designated_ingress","offline_loss_possible":True,"reason":"insufficient_evidence"}}
    fallback = evidence["group"]
    if not image or not re.fullmatch(r"sha256:[0-9a-f]{64}", image) or image != batch.get("capture_image_digest"):
        fallback["reason"] = "target_image_unwitnessed"
        return evidence
    if not actual and batch["kind"] != "synthetic":
        fallback["reason"] = "packet_originals_unverified"
        return evidence
    seq_field = batch["sequence_field"]
    groups, receivers, sessions = defaultdict(set), defaultdict(set), defaultdict(set)
    locators = defaultdict(set)
    paired_packets = defaultdict(list)
    echoes = set()
    for row in batch["observations"]:
        packet = row["packet"]
        if packet.get("message_type") != "group":
            continue
        bot, group = packet.get("self_id"), packet.get("group_id")
        if not bot or not group:
            return evidence
        if packet.get("post_type") == "message_sent":
            if str(packet.get("user_id")) == str(bot):
                echoes.add((str(group),str(bot)))
            continue
        if packet.get("post_type") != "message":
            continue
        label, native, seq = row.get("occurrence"), packet.get("message_id"), packet.get(seq_field)
        if not label or native is None or seq is None or not row.get("session"):
            fallback["reason"] = "missing_independent_label_or_locator"
            return evidence
        topic = packet.get("topic_id")
        key = (str(group), str(topic) if topic is not None else None, str(seq))
        groups[label].add(key)
        receivers[label].add(str(bot))
        sessions[(label,str(bot),str(native))].add(row["session"])
        locators[label].add((str(bot),str(native)))
        paired_packets[label].append(packet)
    if len(groups) < 2 or any(len(keys) != 1 for keys in groups.values()):
        fallback["reason"] = "unpaired_or_conflicting_sequences"
        return evidence
    keys = {k: next(iter(v)) for k,v in sorted(groups.items())}
    common = set.intersection(*receivers.values())
    if len(common) < 2 or len(set(keys.values())) != len(keys):
        fallback["reason"] = "single_receiver_or_occurrence_collision"
        return evidence
    native_owners = {}
    for occurrence, items in locators.items():
        for bot, locator in items:
            inside = (*keys[occurrence][:2],bot,locator)
            if native_owners.setdefault(inside,occurrence) != occurrence:
                fallback["reason"] = "receiver_locator_collision"
                return evidence
    scopes = {key[:2] for key in keys.values()}
    if len(scopes) != 1:
        fallback["reason"] = "test_each_space_and_topic_separately"
        return evidence
    space, topic = next(iter(scopes))
    echo = any((space, bot) in echoes for bot in common)
    mention = False
    for packets in paired_packets.values():
        for p in packets:
            mention |= any(seg.get("type") == "at" and str(seg.get("data",{}).get("qq")) in common
                           for seg in p.get("message",[]) if isinstance(seg,dict))
    # Compare independently labelled occurrences; equal content is a stress case, never the key.
    twins = any(p["time"] == q["time"] and p["message"] == q["message"] and keys[l][:2] == keys[r][:2]
                for l in keys for r in keys if l < r for p in paired_packets[l][:1] for q in paired_packets[r][:1])
    if not (twins and mention and echo and any(len(v) > 1 for v in sessions.values())):
        fallback["reason"] = "missing_twins_mention_echo_or_reconnect"
        return evidence
    evidence["deployment_verified"] = actual
    evidence["group"] = {"mode":"scoped_shared_key","scope":["space","topic",seq_field],
                         "pair_keys":keys,"locators":{k:sorted(v) for k,v in sorted(locators.items())},
                         "applies_to":{"space":space,"topic":topic},
                         "common_receivers":sorted(common),"offline_loss_possible":True}
    return evidence


def load(path):
    batch = json.loads(path.read_bytes())
    if batch["kind"] != "target_capture":
        return assess(batch)
    base = path.resolve().parent
    if "packet" in batch:
        raise ValueError("packet original must belong to an observation")
    for observation in batch["observations"]:
        name = Path(observation["file"])
        source = (base / name).resolve()
        if name.is_absolute() or ".." in name.parts or not source.is_relative_to(base):
            raise ValueError("packet path outside capture")
        raw = source.read_bytes()
        if hashlib.sha256(raw).hexdigest() != observation["sha256"]:
            raise ValueError("packet digest mismatch")
        observation["packet"] = json.loads(raw)
    return assess(batch, originals_verified=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("capture", type=Path)
    args = parser.parse_args()
    print(json.dumps(load(args.capture), indent=2, ensure_ascii=False))
