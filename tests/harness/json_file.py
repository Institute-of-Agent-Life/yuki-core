"""Read one original fixture receipt after its file writer releases it."""
import errno
import json
import os
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


def take_command(path):
    """Only return a command after this peer has claimed it by unlinking its file."""
    try:
        payload = json.loads(path.read_bytes())
        path.unlink()
        return payload
    except FileNotFoundError:
        return None
    except PermissionError as exc:
        if os.name == "nt" and exc.errno == errno.EACCES:
            return None
        raise
