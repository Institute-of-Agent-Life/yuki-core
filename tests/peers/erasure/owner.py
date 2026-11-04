"""A synthetic external owner with its own durable store. Receipts contain no retained body."""

import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
from contextlib import closing
import sys


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("database", type=Path)
    parser.add_argument("action", choices=["retain", "erase", "count"])
    args = parser.parse_args()
    message = json.load(sys.stdin)
    with closing(sqlite3.connect(args.database)) as db:
        db.execute(
            "CREATE TABLE IF NOT EXISTS material(ref TEXT PRIMARY KEY,source INTEGER NOT NULL,body TEXT NOT NULL)"
        )
        db.execute(
            "CREATE TABLE IF NOT EXISTS receipts(id TEXT PRIMARY KEY,watermark INTEGER NOT NULL,proof TEXT NOT NULL)"
        )
        if args.action == "retain":
            db.execute(
                "INSERT INTO material VALUES(?,?,?)",
                (message["ref"], message["source"], json.dumps(message["envelope"])),
            )
        elif args.action == "erase":
            proof = hashlib.sha256(
                json.dumps(
                    [message["key"], message["watermark"], message["sources"]]
                ).encode()
            ).hexdigest()
            for source in message["sources"]:
                db.execute("DELETE FROM material WHERE source=?", (source,))
            db.execute(
                "INSERT OR IGNORE INTO receipts VALUES(?,?,?)",
                (message["key"], message["watermark"], proof),
            )
        db.commit()
        result = {"retained": db.execute("SELECT count(*) FROM material").fetchone()[0]}
        if args.action == "erase":
            result["proof"] = db.execute(
                "SELECT proof FROM receipts WHERE id=?", (message["key"],)
            ).fetchone()[0]
        print(json.dumps(result))


if __name__ == "__main__":
    main()
