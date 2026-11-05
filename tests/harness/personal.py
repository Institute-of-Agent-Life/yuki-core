"""Validate the observable subset of the fixed Client9/Admin OpenAPI using only stdlib."""

import re


def validate(value, schema, document):
    if "$ref" in schema:
        name = schema["$ref"].rsplit("/", 1)[1]
        return validate(value, document["components"]["schemas"][name], document)
    if "anyOf" in schema:
        return any(validate(value, v, document) for v in schema["anyOf"])
    kind = schema.get("type")
    types = {
        "string": str,
        "boolean": bool,
        "integer": int,
        "array": list,
        "object": dict,
        "null": type(None),
    }
    if kind == "number" and type(value) not in (int, float):
        return False
    if kind in types and type(value) is not types[kind]:
        return False
    if "enum" in schema and value not in schema["enum"]:
        return False
    if isinstance(value, list) and "items" in schema:
        return all(validate(item, schema["items"], document) for item in value)
    if isinstance(value, dict):
        if any(k not in value for k in schema.get("required", [])):
            return False
        props = schema.get("properties", {})
        if (
            schema.get("additionalProperties") is False
            and not value.keys() <= props.keys()
        ):
            return False
        return all(
            k not in props or validate(v, props[k], document) for k, v in value.items()
        )
    return True


def matches_reply(docs, method, url, value):
    path = url.partition("?")[0]
    for d in docs:
        for route, methods in d["paths"].items():
            if (
                re.fullmatch(re.sub(r"\{[^}]+\}", r"[^/]+", route), path)
                and method.lower() in methods
            ):
                schema = methods[method.lower()]["responses"]["200"]["content"][
                    "application/json"
                ]["schema"]
                return validate(value, schema, d)
    return path == "/core/v1/entry/exchange"


def template(docs, url):
    path = url.partition("?")[0]
    for d in docs:
        for route in d["paths"]:
            if re.fullmatch(re.sub(r"\{[^}]+\}", r"[^/]+", route), path):
                return route
    if path == "/core/v1/entry/exchange":
        return path
    raise ValueError("route not in fixed source")
