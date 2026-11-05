"""Read one original fixture receipt after its file writer releases it."""
import json
import time


def read_json(path, timeout=40):
    deadline = time.monotonic() + timeout
    while True:
        try:
            return json.loads(path.read_bytes())
        except (FileNotFoundError, PermissionError, json.JSONDecodeError):
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.01)
