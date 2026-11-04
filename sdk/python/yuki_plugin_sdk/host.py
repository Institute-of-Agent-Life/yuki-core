"""Opt-in RPC for the host candidate. Only methods negotiated on this connection are callable."""

import json
from importlib.resources import files

SCHEMA = json.loads(files(__package__).joinpath("host-preview.json").read_bytes())


class Calls:
    def __init__(self, initialized):
        if initialized.get("host_schema_revision") != SCHEMA["schema_revision"]:
            raise ValueError("host preview not negotiated")
        self.allowed = {
            m["name"]
            for m in initialized["method_revisions"]
            if SCHEMA["methods"].get(m["name"], {}).get("revision") == m["revision"]
        }
        self.pending = {}

    def request(self, request_id, method, params):
        if (
            type(request_id) not in (str, int)
            or request_id in self.pending
            or len(self.pending) >= 128
            or method not in self.allowed
            or SCHEMA["methods"][method]["direction"] != "plugin_to_host"
        ):
            raise ValueError("unavailable method or request ID")
        frame = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
        if len(json.dumps(frame, ensure_ascii=False).encode()) > 65536:
            raise ValueError("frame too large")
        self.pending[request_id] = method
        return frame

    def receive(self, frame):
        if frame.get("jsonrpc") != "2.0":
            raise ValueError("invalid JSON-RPC")
        method = frame.get("method")
        if method is not None:
            if method not in self.allowed or "result" in frame or "error" in frame:
                raise ValueError("undeclared or mixed request")
            direction = SCHEMA["methods"][method]["direction"]
            if direction == "host_notification" and "id" not in frame:
                return "notification", method, frame["params"]
            if direction != "host_to_plugin" or type(frame.get("id")) not in (str, int):
                raise ValueError("wrong call direction")
            return "call", method, frame["params"]
        if (
            ("result" in frame) == ("error" in frame)
            or type(frame.get("id")) not in (str, int)
            or frame["id"] not in self.pending
        ):
            raise ValueError("unmatched response")
        return "response", self.pending.pop(frame["id"]), frame
