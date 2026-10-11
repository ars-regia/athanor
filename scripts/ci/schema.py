"""The subset of JSON Schema that the schemas of scripts/ci/schemas use, checked with the
standard library: jsonschema is not a dependency of the pipeline. A keyword outside the subset
fails, so a schema never appears to say more than is checked."""

import json
import pathlib
import re

SCHEMAS = pathlib.Path(__file__).resolve().parent / "schemas"
KEYWORDS = {
    "$schema",
    "$id",
    "title",
    "description",
    "type",
    "required",
    "properties",
    "additionalProperties",
    "items",
    "pattern",
    "const",
    "enum",
    "minimum",
}
TYPES = {
    "object": dict,
    "array": list,
    "string": str,
    "integer": int,
    "null": type(None),
}


def load(name):
    return json.loads((SCHEMAS / f"{name}.schema.json").read_text())


def errors(doc, schema, at="$"):
    unknown = sorted(set(schema) - KEYWORDS)
    if unknown or schema.get("additionalProperties", False) is not False:
        return [
            f"{at}: schema uses {unknown or ['additionalProperties other than false']}, outside the subset"
        ]
    kinds = schema.get("type", [])
    kinds = [kinds] if isinstance(kinds, str) else kinds
    if kinds and not any(
        isinstance(doc, TYPES[k]) and not isinstance(doc, bool) for k in kinds
    ):
        return [f"{at}: not {' or '.join(kinds)}"]
    out = []
    if "const" in schema and doc != schema["const"]:
        out.append(f"{at}: not {schema['const']!r}")
    if "enum" in schema and doc not in schema["enum"]:
        out.append(f"{at}: not in enum {schema['enum']}")
    if (
        "pattern" in schema
        and isinstance(doc, str)
        and not re.search(schema["pattern"], doc)
    ):
        out.append(f"{at}: does not match pattern {schema['pattern']}")
    if "minimum" in schema and isinstance(doc, int) and doc < schema["minimum"]:
        out.append(f"{at}: below minimum {schema['minimum']}")
    if isinstance(doc, dict):
        props = schema.get("properties", {})
        out += [
            f"{at}.{k}: missing" for k in schema.get("required", []) if k not in doc
        ]
        if "additionalProperties" in schema:
            out += [f"{at}.{k}: not allowed" for k in doc if k not in props]
        for k, v in doc.items():
            if k in props:
                out += errors(v, props[k], f"{at}.{k}")
    if isinstance(doc, list) and "items" in schema:
        for i, v in enumerate(doc):
            out += errors(v, schema["items"], f"{at}[{i}]")
    return out


def validate(doc, schema):
    problems = errors(doc, schema)
    if problems:
        raise ValueError("; ".join(problems))
