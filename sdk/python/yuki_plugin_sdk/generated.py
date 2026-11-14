"""Generated from handshake.schema.json. Run protocol/plugin/generate.py to update."""
from dataclasses import dataclass, field

@dataclass
class Version:
    major: int
    minor: int


@dataclass
class VersionRange:
    min: Version
    max: Version


@dataclass
class Declaration:
    kind: str
    name: str
    revision: int


@dataclass
class MethodRevision:
    name: str
    revision: int


@dataclass
class Initialize:
    versions: VersionRange
    required_features: list[str]
    manifest_revision: str
    declarations: list[Declaration]
    optional_features: list[str] = field(default_factory=list)


@dataclass
class Initialized:
    version: Version
    features: list[str]
    extension_points: list[str]
    method_revisions: list[MethodRevision]
    schema_revision: int
    namespace: str
    grants: list[str]
    manifest_revision: str


CONTRACT = {'status': 'unpublished-handshake-preview', 'schema_revision': 1, 'sdk_versions': {'rust': '1.0.0-alpha.26', 'python': '1.0.0a26'}, 'versions': [{'major': 1, 'minor': 0}], 'features': ['initialize'], 'extension_points': [], 'max_frame_bytes': 65536, 'methods': {'initialize': {'revision': 1, 'params': 'Initialize', 'result': 'Initialized'}}, 'errors': {'unsupported_version': -32010, 'missing_feature': -32011, 'manifest_mismatch': -32012, 'duplicate_declaration': -32013, 'method_not_found': -32601, 'invalid_request': -32600, 'invalid_params': -32602, 'parse_error': -32700}}
