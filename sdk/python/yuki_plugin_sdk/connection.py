"""Bound one connection to its host's approved manifest and isolate its request IDs."""

from dataclasses import asdict, dataclass
from importlib.resources import files
import json

from . import generated as g

SCHEMA = json.loads(files(__package__).joinpath("handshake.schema.json").read_bytes())
META = g.CONTRACT
ERRORS = META["errors"]


def decode(shape, value):
    if "$ref" in shape:
        name = shape["$ref"].split("/")[-1]
        fields = decode(SCHEMA["$defs"][name], value)
        return getattr(g, name)(**fields)
    kind = shape["type"]
    if kind == "object":
        if not isinstance(value, dict) or not set(shape["required"]) <= value.keys():
            raise ValueError("invalid object or missing required field")
        return {
            name: decode(rule, value[name])
            for name, rule in shape["properties"].items()
            if name in value
        }
    if kind == "array" and type(value) is list and len(value) <= shape["maxItems"]:
        return [decode(shape["items"], item) for item in value]
    if (
        kind == "integer"
        and type(value) is int
        and shape["minimum"] <= value <= shape["maximum"]
    ):
        return value
    if (
        kind == "string"
        and type(value) is str
        and shape["minLength"] <= len(value) <= shape["maxLength"]
    ):
        return value
    raise ValueError("invalid " + kind)


def model(name, value):
    return decode({"$ref": "#/$defs/" + name}, value)


def encode(frame):
    data = json.dumps(
        frame, ensure_ascii=False, separators=(",", ":"), allow_nan=False
    ).encode()
    if len(data) > META["max_frame_bytes"]:
        raise ValueError("frame too large")
    return data + b"\n"


def read_frame(stream):
    line = stream.readline(META["max_frame_bytes"] + 2)
    if not line:
        return None
    if not line.endswith(b"\n") or len(line) - 1 > META["max_frame_bytes"]:
        raise ValueError("oversized or unterminated frame")
    return line[:-1]


def _version(v):
    return (v.major, v.minor)


def _ordered(declarations):
    return sorted(declarations, key=lambda d: d.name)


@dataclass(frozen=True)
class Binding:
    namespace: str
    grants: tuple[str, ...]
    manifest_revision: str
    declarations: tuple[g.Declaration, ...]

    @classmethod
    def from_json(cls, raw):
        if not isinstance(raw,dict) or not isinstance(raw.get("grants"),list) or not isinstance(raw.get("declarations"),list) or raw.keys() != {"namespace", "grants", "manifest_revision", "declarations"}:
            raise ValueError("invalid trusted binding")
        return cls(
            raw["namespace"],
            tuple(raw["grants"]),
            raw["manifest_revision"],
            tuple(model("Declaration", d) for d in raw["declarations"]),
        )

    def offer(self):
        versions = [g.Version(**v) for v in META["versions"]]
        return g.Initialize(
            versions=g.VersionRange(
                min(versions, key=_version), max(versions, key=_version)
            ),
            required_features=list(META["features"]),
            manifest_revision=self.manifest_revision,
            declarations=list(self.declarations),
        )


@dataclass
class Received:
    reply: dict | None = None
    response_id: int | str | None = None
    result: g.Initialized | None = None
    error: dict | None = None


def _reject(rpc_id, reason):
    return Received(
        reply={
            "jsonrpc": "2.0",
            "id": rpc_id,
            "error": {"code": ERRORS[reason], "message": reason},
        }
    )


def valid_id(value):
    return type(value) is str or (type(value) is int and -(2**63) <= value < 2**63)


class Peer:
    def __init__(self, binding):
        if (
            not isinstance(binding, Binding)
            or not binding.namespace.strip()
            or len(binding.namespace) > 256
        ):
            raise ValueError("invalid trusted binding")
        self._binding = Binding.from_json(
            {
                "namespace": binding.namespace,
                "grants": list(binding.grants),
                "manifest_revision": binding.manifest_revision,
                "declarations": [asdict(d) for d in binding.declarations],
            }
        )
        model("Initialize", asdict(self._binding.offer()))
        if len({d.name for d in binding.declarations}) != len(binding.declarations):
            raise ValueError("duplicate trusted declaration")
        if len(binding.grants) > 128 or any(
            type(grant) is not str or not 1 <= len(grant) <= 256
            for grant in binding.grants
        ):
            raise ValueError("invalid trusted grant")
        self._pending = {}
        self._negotiated = None

    def request(self, rpc_id, params):
        if not valid_id(rpc_id) or rpc_id in self._pending or len(self._pending) >= 128:
            raise ValueError("invalid or duplicate pending request")
        validated = model("Initialize", asdict(params))
        frame = {
            "jsonrpc": "2.0",
            "id": rpc_id,
            "method": "initialize",
            "params": asdict(validated),
        }
        encode(frame)
        self._pending[rpc_id] = validated
        return frame

    def _accept(self, rpc_id, raw):
        try:
            params = model("Initialize", raw)
        except (ValueError, TypeError):
            return _reject(rpc_id, "invalid_params")
        binding = self._binding
        if len({d.name for d in params.declarations}) != len(params.declarations):
            return _reject(rpc_id, "duplicate_declaration")
        if params.manifest_revision != binding.manifest_revision or _ordered(
            params.declarations
        ) != _ordered(binding.declarations):
            return _reject(rpc_id, "manifest_mismatch")
        versions = [
            v
            for v in (g.Version(**v) for v in META["versions"])
            if _version(params.versions.min)
            <= _version(v)
            <= _version(params.versions.max)
        ]
        if not versions:
            return _reject(rpc_id, "unsupported_version")
        if not set(params.required_features) <= set(META["features"]):
            return _reject(rpc_id, "missing_feature")
        selected = sorted(
            set(params.required_features + params.optional_features)
            & set(META["features"])
        )
        result = g.Initialized(
            version=max(versions, key=_version),
            features=selected,
            extension_points=META["extension_points"].copy(),
            method_revisions=[
                g.MethodRevision(k, v["revision"]) for k, v in META["methods"].items()
            ],
            schema_revision=META["schema_revision"],
            namespace=binding.namespace,
            grants=list(binding.grants),
            manifest_revision=binding.manifest_revision,
        )
        if self._negotiated is not None and self._negotiated != result:
            return _reject(rpc_id, "manifest_mismatch")
        self._negotiated = result
        return Received(
            reply={"jsonrpc": "2.0", "id": rpc_id, "result": asdict(result)}
        )

    def receive(self, data):
        if len(data) > META["max_frame_bytes"]:
            raise ValueError("frame too large")
        try:
            frame = json.loads(data.decode("utf-8"))
        except (ValueError, UnicodeError):
            return _reject(None, "parse_error")
        if not isinstance(frame, dict) or frame.get("jsonrpc") != "2.0":
            return _reject(None, "invalid_request")
        rpc_id = frame.get("id")
        if "method" in frame:
            if not isinstance(frame["method"], str):
                return _reject(None, "invalid_request")
            if "id" not in frame:
                return Received()
            if not valid_id(rpc_id) or "result" in frame or "error" in frame:
                return _reject(None, "invalid_request")
            if frame["method"] not in META["methods"]:
                return _reject(rpc_id, "method_not_found")
            return self._accept(rpc_id, frame.get("params"))
        if not valid_id(rpc_id) or rpc_id not in self._pending:
            raise ValueError("unknown response ID")
        if ("result" in frame) == ("error" in frame):
            raise ValueError("malformed response")
        offer = self._pending[rpc_id]
        if "result" in frame:
            result = model("Initialized", frame["result"])
            if (
                not _version(offer.versions.min)
                <= _version(result.version)
                <= _version(offer.versions.max)
                or result.schema_revision != META["schema_revision"]
                or result.manifest_revision != offer.manifest_revision
                or not set(offer.required_features)
                <= set(result.features)
                <= set(offer.required_features + offer.optional_features)
                or not any(
                    m.name == "initialize"
                    and m.revision == META["methods"]["initialize"]["revision"]
                    for m in result.method_revisions
                )
            ):
                raise ValueError("result outside offered contract")
            response = Received(response_id=rpc_id, result=result)
        else:
            err = frame["error"]
            if (
                not isinstance(err, dict)
                or type(err.get("code")) is not int
                or type(err.get("message")) is not str
            ):
                raise ValueError("invalid response error")
            response = Received(response_id=rpc_id, error=err)
        del self._pending[rpc_id]
        return response

    def finish(self):
        if self._pending:
            raise ValueError("EOF with pending requests")
