"""Report owner facts from a declared offline snapshot. Never open the source DB in SQLite."""
import argparse
import hashlib
import json
import re
import shutil
import sqlite3
import tempfile
from contextlib import closing
from email.parser import BytesParser
from pathlib import Path, PureWindowsPath


def sha_file(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def bounded(root, name):
    path = Path(name)
    if path.is_absolute() or PureWindowsPath(name).drive or ".." in path.parts or not path.parts:
        raise ValueError("invalid snapshot-relative reference")
    dest = root / path
    if dest.resolve() != dest or not dest.is_relative_to(root):
        raise ValueError("snapshot reference leaves owner root")
    return dest


def key_digest(keys):
    digest = hashlib.sha256()
    for key in keys:
        values = [dict(blob=value.hex()) if isinstance(value, bytes) else value for value in key]
        data = json.dumps(values, ensure_ascii=False, separators=(",", ":")).encode()
        digest.update(len(data).to_bytes(8, "big") + data)
    return digest.hexdigest()


def quoted(name):
    return '"' + name.replace('"', '""') + '"'


def collect(root):
    root = root.resolve()
    meta = json.loads(bounded(root, "snapshot.json").read_bytes())
    if meta["capture_method"] not in {"quiesced", "sqlite_backup"} or not meta["owner_barrier"]:
        raise ValueError("a cross-owner capture barrier is required")
    rows, versions, files = [], {}, {}
    def row(owner, kind, ref, count, keys, status="observed", reason=None):
        rows.append({"snapshot_id": meta["snapshot_id"], "owner": owner, "original_class": kind,
                     "count": count, "stable_key_digest": keys, "source_ref": ref, "target_ref": None,
                     "status": status, "exclude_reason": reason, "privacy_watermark": meta["privacy_watermark"],
                     "effect_delta": 0})

    def referenced(owner, folder, identifier, name, expected, size):
        blob = bounded(folder, name)
        status, reason = "observed", None
        if not blob.is_file():
            status, reason = "missing", "referenced bytes absent"
        elif blob.stat().st_size != size or (expected and sha_file(blob) != expected):
            status, reason = "mismatch", "bytes disagree with owner manifest"
        elif not expected:
            status, reason = "unverified", "source digest absent"
        row(owner, "artifact_bytes", blob.relative_to(root).as_posix(), int(blob.is_file()),
            key_digest([(identifier,)]), status, reason)

    def inventory_db(db, owner, ref, folder):
        table_info = db.execute("PRAGMA table_list").fetchall()
        for _, name, kind, *_ in sorted(table_info, key=lambda t: t[1]):
            if name.startswith("sqlite_") or kind == "view":
                continue
            if kind in ("shadow", "virtual"):
                row(owner, name, ref, None, None, "not_scanned", "virtual index or native module")
                continue
            table = quoted(name)
            columns = db.execute(f"PRAGMA table_info({table})").fetchall()
            primary = [quoted(col[1]) for col in sorted(columns, key=lambda c: c[5]) if col[5]]
            count = db.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
            if primary:
                selection = ", ".join(primary)
                keys = key_digest(db.execute(f"SELECT {selection} FROM {table} ORDER BY {selection}"))
                row(owner, name, ref, count, keys)
            else:
                row(owner, name, ref, count, None, "unmapped", "no declared stable primary key")
        violations = db.execute("PRAGMA foreign_key_check").fetchall()
        row(owner, "foreign_key_violations", ref, len(violations), key_digest(sorted(violations)),
            "mismatch" if violations else "observed", "orphaned foreign keys" if violations else None)
        tables = {t[1] for t in table_info}
        if owner == "core":
            revisions = db.execute("SELECT version_num FROM alembic_version").fetchall() if "alembic_version" in tables else []
            versions["database"] = revisions[0][0] if len(revisions) == 1 else None
            if {"persons", "account_assignments"} <= tables:
                ids = db.execute("SELECT p.id FROM persons p WHERE NOT EXISTS "
                    "(SELECT 1 FROM account_assignments a WHERE a.person_id=p.id) ORDER BY p.id").fetchall()
                row(owner, "persons_without_account", ref, len(ids), key_digest(ids))
            if "tool_artifacts" in tables:
                object_root = meta["file_roots"].get("tool-artifacts")
                if object_root:
                    for fields in db.execute("SELECT handle_id,relative_path,sha256,byte_size FROM tool_artifacts ORDER BY handle_id"):
                        referenced(owner, bounded(root, object_root), *fields)
                else:
                    row(owner, "tool_artifact_closure", ref, None, None, "unverified", "tool-artifacts owner root not supplied")
        if owner == "workspace" and "artifacts" in tables:
            for fields in db.execute("SELECT id,blob,sha256,size FROM artifacts ORDER BY id"):
                referenced(owner, folder, *fields)

    for owner, entry in sorted(meta["databases"].items()):
        name = entry["path"]
        original = bounded(root, name)
        if not original.is_file() and entry["db_sha256"] is None and not bounded(root, name + "-wal").exists() and entry["wal_sha256"] is None:
            row(owner, "database", name, None, None, "missing", "owner snapshot unavailable")
            continue
        with tempfile.TemporaryDirectory(prefix="v4-owner-") as tmp:
            copy = Path(tmp) / "snapshot.db"
            for suffix, field in (("", "db_sha256"), ("-wal", "wal_sha256")):
                source = bounded(root, name + suffix)
                destination = Path(str(copy) + suffix)
                expected = entry[field]
                if expected is None and not source.exists():
                    continue
                if not expected or not source.is_file() or sha_file(source) != expected:
                    raise ValueError("snapshot bytes differ from capture manifest: " + name + suffix)
                shutil.copyfile(source, destination)
                if sha_file(destination) != expected:
                    raise ValueError("snapshot changed while copying")
                files[name + suffix] = expected
            with closing(sqlite3.connect(copy.as_uri() + "?mode=ro", uri=True)) as db:
                db.execute("PRAGMA query_only=ON")
                db.execute("PRAGMA trusted_schema=OFF")
                inventory_db(db, owner, name, original.parent)
    for owner, directory in sorted(meta["file_roots"].items()):
        start = bounded(root, directory)
        if not start.is_dir():
            row(owner, "files", directory, None, None, "missing", "owner root unavailable")
            continue
        manifest = []
        for path in sorted(start.rglob("*")):
            bounded(start, path.relative_to(start).as_posix())
            if path.is_file():
                file_hash = sha_file(path)
                files[path.relative_to(root).as_posix()] = file_hash
                manifest.append((path.relative_to(start).as_posix(), file_hash))
        row(owner, "files", directory, len(manifest), key_digest(manifest))
    ns = sorted(meta["credential_namespaces"])
    row("credentials", "namespaces", "owner declaration", len(ns), key_digest((n,) for n in ns),
        "excluded_values", "namespace names only")
    versions["documents"] = {}
    for label, name, expression in [
        ("changelog", "changelog", r"(?:数据库 head 为|database head) \*\*(\d{4})"),
        ("readme_schema", "readme", r"release-baseline: [^>]*schema=(\d{4})"),
        ("readme_connector", "readme", r"本机连接器 \*\*([0-9.]+) /"),
    ]:
        path = bounded(root, meta["documents"][name]) if name in meta["documents"] else None
        match = re.search(expression, path.read_text(encoding="utf-8")) if path and path.is_file() else None
        versions["documents"][label] = match.group(1) if match else None
    package = bounded(root, meta["connector_metadata"]) if meta.get("connector_metadata") else None
    package_info = BytesParser().parsebytes(package.read_bytes()) if package and package.is_file() else {}
    versions["connector_distribution"] = package_info.get("Version") if package_info.get("Name") == "yuki-agent-connector" else None
    image = bounded(root, meta["image_manifest"]) if meta.get("image_manifest") else None
    versions["image_digest"] = "sha256:" + sha_file(image) if image and image.is_file() else None
    versions["deployment_image_ref"] = meta.get("deployment_image_digest")
    versions["image_binding"] = ("matched" if versions["image_digest"] and
        versions["image_digest"] == versions["deployment_image_ref"] else "unverified")
    for name, expected in files.items():
        if sha_file(bounded(root, name)) != expected:
            raise ValueError("source changed during inventory")
    return {"snapshot_id": meta["snapshot_id"], "kind": meta["kind"], "versions": versions, "rows": rows,
            "privacy_watermark": meta["privacy_watermark"], "captured_files": files,
            "deployment": {"status": "awaiting_authorized_snapshot" if meta["kind"] == "synthetic" else "snapshot_inspected"}}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--snapshot", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.out.resolve().is_relative_to(args.snapshot.resolve()):
        parser.error("report destination inside source snapshot")
    result = collect(args.snapshot)
    with args.out.open("x", encoding="utf-8") as report:
        json.dump(result, report, indent=2, ensure_ascii=False)
        report.write("\n")


if __name__ == "__main__":
    main()
