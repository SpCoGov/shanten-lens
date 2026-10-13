# Copyright 2026 Shinkuan (Akagi). Licensed under Apache-2.0; see LICENSE.
# Adapted from SpCoGov/Akagi@3df41dcf80532fce546570fe06bcfd54ce3a4ec1.
# Modified to extract shanten-lens registries and fail on unresolved proto fields.
"""Unity client download and pure liqi descriptor reconstruction."""

from __future__ import annotations

import json
import posixpath
import re
import warnings
from dataclasses import dataclass, field
from urllib.parse import urljoin, urlsplit

try:  # only needed for the Unity download path; offline mode works without it
    import requests
except ModuleNotFoundError:  # pragma: no cover
    requests = None


CLIENT_URL = "https://game.maj-soul.com/1/"
TEXTURE_PROFILES = ("ASTC", "DXT")
LUA_XOR_KEY = b"wrelupqezdfrqdsd"
UNITY_FALLBACK_VERSION = "2022.3.62f2c1"
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
)

PROTOL_PREFIX = "LuaByte/Lua/Protol/"
PROTO_CONFIG_ASSET = "MyAssets/docs/proto_config.bytes"

# Wire-protocol package. `lq.config` and `lqc` (excel) are game-data, excluded.
WIRE_PACKAGE = "lq"

# FieldDescriptorProto.Type numbering (descriptor.proto), as emitted in the Lua.
PROTO_TYPE_NAMES = {
    1: "double", 2: "float", 3: "int64", 4: "uint64", 5: "int32",
    6: "fixed64", 7: "fixed32", 8: "bool", 9: "string", 10: "group",
    11: "message", 12: "bytes", 13: "uint32", 14: "enum", 15: "sfixed32",
    16: "sfixed64", 17: "sint32", 18: "sint64",
}
LABEL_REPEATED = 3

# If any of these is missing after extraction, the run fails loudly rather than
# writing a truncated schema.
REQUIRED_MESSAGES = (
    "ResAuthGame", "ActionPrototype", "ReqSelfOperation", "ResSyncGame",
    "RecordNewRound", "ReqChiPengGang", "OptionalOperationList", "Wrapper",
    "ResEnterGame", "GameEndResult",
)


# --------------------------------------------------------------------------- #
# Model
# --------------------------------------------------------------------------- #
@dataclass
class Field:
    name: str
    number: int
    label: int
    type_code: int
    index: int = 0
    # Raw Lua reference to a message/enum type: "b.ERROR" or a local var name.
    type_ref: str | None = None
    # Resolved fully-qualified type name, e.g. ".lq.Error".
    type_fqn: str | None = None

    @property
    def repeated(self) -> bool:
        return self.label == LABEL_REPEATED


@dataclass
class EnumValue:
    name: str
    number: int
    index: int = 0


@dataclass
class Enum:
    var: str
    name: str
    full_name: str
    values: list[EnumValue] = field(default_factory=list)


@dataclass
class Message:
    var: str
    name: str
    full_name: str
    fields: list[Field] = field(default_factory=list)
    nested_messages: list["Message"] = field(default_factory=list)
    nested_enums: list[Enum] = field(default_factory=list)
    has_containing_type: bool = False


@dataclass
class Module:
    name: str          # e.g. "com_struct_pb"
    package: str        # e.g. "lq"
    imports: dict[str, str]                    # alias -> module name
    messages_by_var: dict[str, Message]
    enums_by_var: dict[str, Enum]
    top_messages: list[Message]
    top_enums: list[Enum]


# --------------------------------------------------------------------------- #
# Lua ProtoDeclare parsing (pure)
# --------------------------------------------------------------------------- #
_DECL_RE = {
    "message": re.compile(r"(\w+)\s*=\s*\w+\.Descriptor\(\)"),
    "enum": re.compile(r"(\w+)\s*=\s*\w+\.EnumDescriptor\(\)"),
    "enum_value": re.compile(r"(\w+)\s*=\s*\w+\.EnumValueDescriptor\(\)"),
    "field": re.compile(r"(\w+)\s*=\s*\w+\.FieldDescriptor\(\)"),
}
_IMPORT_RE = re.compile(r"local\s+(\w+)\s*=\s*require\(?[\"'](Protol\.[^\"']+)[\"']\)?")
_STR_PROP_RE = re.compile(r"(\w+)\.(\w+)\s*=\s*\"((?:\\.|[^\"\\])*)\"")
_NUM_PROP_RE = re.compile(r"(\w+)\.(\w+)\s*=\s*(-?\d+)(?![\w.])")
_REF_PROP_RE = re.compile(r"(\w+)\.(message_type|enum_type|containing_type)\s*=\s*(\w+(?:\.\w+)?)")
_LIST_PROP_RE = re.compile(r"(\w+)\.(fields|nested_types|enum_types|values)\s*=\s*\{")


def _find_matching_brace(text: str, open_idx: int) -> int:
    depth, i, quote, escaped = 0, open_idx, None, False
    while i < len(text):
        c = text[i]
        if quote:
            if escaped:
                escaped = False
            elif c == "\\":
                escaped = True
            elif c == quote:
                quote = None
        elif c in "\"'":
            quote = c
        elif c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise ValueError(f"unbalanced brace at {open_idx}")


def _unescape(value: str) -> str:
    return re.sub(
        r"\\(\d{1,3}|.)",
        lambda m: chr(int(m.group(1))) if m.group(1).isdigit()
        else {"n": "\n", "r": "\r", "t": "\t", "\\": "\\", '"': '"', "'": "'"}.get(
            m.group(1), m.group(1)
        ),
        value,
    )


def parse_lua_module(module_name: str, source: str) -> Module:
    """Parse one `Protol.*_pb.lua` descriptor script into a Module."""
    kinds: dict[str, str] = {}
    for kind, rx in _DECL_RE.items():
        for var in rx.findall(source):
            # A var is declared once; first declaration wins its kind.
            kinds.setdefault(var, kind)

    props: dict[str, dict[str, object]] = {v: {} for v in kinds}

    for var, prop, val in _STR_PROP_RE.findall(source):
        if var in props:
            props[var][prop] = _unescape(val)
    for var, prop, val in _NUM_PROP_RE.findall(source):
        if var in props and prop not in props[var]:
            props[var][prop] = int(val)
    for var, prop, val in _REF_PROP_RE.findall(source):
        if var in props:
            props[var][prop] = val
    for m in _LIST_PROP_RE.finditer(source):
        var, prop = m.group(1), m.group(2)
        if var not in props:
            continue
        open_idx = m.end() - 1
        close_idx = _find_matching_brace(source, open_idx)
        props[var][prop] = re.findall(r"\w+", source[open_idx + 1:close_idx])

    imports = {alias: mod.split(".", 1)[1] for alias, mod in _IMPORT_RE.findall(source)}

    enums_by_var: dict[str, Enum] = {}
    for var, kind in kinds.items():
        if kind != "enum":
            continue
        p = props[var]
        if not isinstance(p.get("name"), str) or not isinstance(p.get("full_name"), str):
            continue
        enum = Enum(var=var, name=p["name"], full_name=p["full_name"])
        for vv in p.get("values", []):
            if kinds.get(vv) != "enum_value":
                continue
            vp = props[vv]
            if isinstance(vp.get("name"), str) and isinstance(vp.get("number"), int):
                enum.values.append(
                    EnumValue(vp["name"], vp["number"], int(vp.get("index", 0)))
                )
        enum.values.sort(key=lambda x: x.index)
        enums_by_var[var] = enum

    messages_by_var: dict[str, Message] = {}
    for var, kind in kinds.items():
        if kind != "message":
            continue
        p = props[var]
        if not isinstance(p.get("name"), str) or not isinstance(p.get("full_name"), str):
            continue
        messages_by_var[var] = Message(
            var=var, name=p["name"], full_name=p["full_name"],
            has_containing_type=isinstance(p.get("containing_type"), str),
        )

    for var, msg in messages_by_var.items():
        p = props[var]
        for fv in p.get("fields", []):
            if kinds.get(fv) != "field":
                continue
            fp = props[fv]
            if not all(isinstance(fp.get(k), int) for k in ("number", "label", "type")):
                continue
            if not isinstance(fp.get("name"), str):
                continue
            msg.fields.append(Field(
                name=fp["name"], number=fp["number"], label=fp["label"],
                type_code=fp["type"], index=int(fp.get("index", 0)),
                type_ref=fp.get("message_type") or fp.get("enum_type"),
            ))
        msg.fields.sort(key=lambda x: x.number)
        for nv in p.get("nested_types", []):
            if nv in messages_by_var:
                msg.nested_messages.append(messages_by_var[nv])
        for ev in p.get("enum_types", []):
            if ev in enums_by_var:
                msg.nested_enums.append(enums_by_var[ev])

    nested_msg_vars = {n.var for m in messages_by_var.values() for n in m.nested_messages}
    nested_enum_vars = {e.var for m in messages_by_var.values() for e in m.nested_enums}
    top_messages = [
        m for v, m in messages_by_var.items()
        if v not in nested_msg_vars and not m.has_containing_type
    ]
    top_enums = [e for v, e in enums_by_var.items() if v not in nested_enum_vars]

    package = ""
    for item in list(messages_by_var.values()) + list(enums_by_var.values()):
        parts = item.full_name.lstrip(".").split(".")
        if len(parts) > 1:
            package = ".".join(parts[:-1])
            break

    return Module(
        name=module_name, package=package, imports=imports,
        messages_by_var=messages_by_var, enums_by_var=enums_by_var,
        top_messages=top_messages, top_enums=top_enums,
    )


def resolve_types(modules: dict[str, Module]) -> None:
    """Resolve each field's message/enum ref to a fully-qualified `.lq.X` name."""
    for module in modules.values():
        for msg in module.messages_by_var.values():
            for fld in msg.fields:
                if fld.type_ref:
                    fld.type_fqn = _resolve_ref(fld.type_ref, module, modules)


def _resolve_ref(ref: str, module: Module, modules: dict[str, Module]) -> str | None:
    if "." not in ref:
        target = module.messages_by_var.get(ref) or module.enums_by_var.get(ref)
        return target.full_name if target else None
    alias, var = ref.split(".", 1)
    other = modules.get(module.imports.get(alias, ""))
    if not other:
        return None
    target = other.messages_by_var.get(var) or other.enums_by_var.get(var)
    return target.full_name if target else None


# --------------------------------------------------------------------------- #
# proto3 rendering (pure)
# --------------------------------------------------------------------------- #
def _proto_type(fld: Field) -> str:
    if fld.type_code in (11, 14):          # message / enum
        if not fld.type_fqn:
            raise ValueError(f"unresolved protobuf field: {fld.name} ({fld.type_ref})")
        return fld.type_fqn
    if fld.type_code not in PROTO_TYPE_NAMES or fld.type_code == 10:
        raise ValueError(f"unsupported protobuf field type: {fld.type_code}")
    return PROTO_TYPE_NAMES[fld.type_code]


def _render_enum(enum: Enum, indent: int, out: list[str]) -> None:
    pad = " " * indent
    out.append(f"{pad}enum {enum.name} {{")
    numbers = [v.number for v in enum.values]
    if len(numbers) != len(set(numbers)):
        out.append(f"{pad}    option allow_alias = true;")
    for v in enum.values:
        out.append(f"{pad}    {v.name} = {v.number};")
    out.append(f"{pad}}}")


def _render_message(msg: Message, indent: int, out: list[str]) -> None:
    pad = " " * indent
    out.append(f"{pad}message {msg.name} {{")
    for enum in msg.nested_enums:
        _render_enum(enum, indent + 4, out)
    for nested in sorted(msg.nested_messages, key=lambda m: m.name):
        _render_message(nested, indent + 4, out)
    for fld in sorted(msg.fields, key=lambda f: f.number):
        prefix = "repeated " if fld.repeated else ""
        out.append(f"{pad}    {prefix}{_proto_type(fld)} {fld.name} = {fld.number};")
    out.append(f"{pad}}}")


def render_proto(modules: dict[str, Module], services: dict[str, dict]) -> str:
    """Render one flat proto3 file for the wire package, plus service blocks."""
    wire = [m for m in modules.values() if m.package == WIRE_PACKAGE]

    seen: dict[str, str] = {}
    enums: list[Enum] = []
    messages: list[Message] = []
    for module in wire:
        for enum in module.top_enums:
            if enum.full_name in seen:
                raise ValueError(f"duplicate enum {enum.full_name} in {module.name}/{seen[enum.full_name]}")
            seen[enum.full_name] = module.name
            enums.append(enum)
        for msg in module.top_messages:
            if msg.full_name in seen:
                raise ValueError(f"duplicate message {msg.full_name} in {module.name}/{seen[msg.full_name]}")
            seen[msg.full_name] = module.name
            messages.append(msg)

    lines = ['syntax = "proto3";', "", f"package {WIRE_PACKAGE};", ""]
    for enum in sorted(enums, key=lambda e: e.full_name):
        _render_enum(enum, 0, lines)
        lines.append("")
    for msg in sorted(messages, key=lambda m: m.full_name):
        _render_message(msg, 0, lines)
        lines.append("")

    for service_name in sorted(services):
        lines.append(f"service {service_name} {{")
        for method in sorted(services[service_name]):
            spec = services[service_name][method]
            lines.append(
                f"    rpc {method} ({spec['request']}) returns ({spec['response']});"
            )
        lines.append("}")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def filter_services(
    services: dict[str, dict], message_names: set[str]
) -> tuple[dict[str, dict], list[str]]:
    """Drop methods whose request/response type is not in the message set.

    `proto_config.bytes` lists internal debug/test RPCs (e.g. *ActivityDebug)
    whose message types are stripped from the shipped descriptors. Those never
    occur in real traffic, so they are skipped (and reported) rather than
    breaking proto compilation or the rpc-map.
    """
    resolved: dict[str, dict] = {}
    skipped: list[str] = []
    for service_name in sorted(services):
        keep: dict[str, dict] = {}
        for method in sorted(services[service_name]):
            spec = services[service_name][method]
            if spec["request"] in message_names and spec["response"] in message_names:
                keep[method] = spec
            else:
                skipped.append(f"{service_name}.{method}")
        if keep:
            resolved[service_name] = keep
    return resolved, skipped


def build_rpc_map(services: dict[str, dict]) -> dict[str, dict]:
    """Flat rpc-map keyed by `.lq.Service.method` (services already resolved)."""
    rpc_map: dict[str, dict] = {}
    for service_name in sorted(services):
        for method in sorted(services[service_name]):
            spec = services[service_name][method]
            rpc_map[f".{WIRE_PACKAGE}.{service_name}.{method}"] = {
                "req": f".{WIRE_PACKAGE}.{spec['request']}",
                "resp": f".{WIRE_PACKAGE}.{spec['response']}",
            }
    return rpc_map


def collect_message_names(modules: dict[str, Module]) -> set[str]:
    names: set[str] = set()
    for module in modules.values():
        if module.package != WIRE_PACKAGE:
            continue
        for msg in module.messages_by_var.values():
            if not msg.has_containing_type:
                names.add(msg.name)
    return names


def build_outputs(
    lua_sources: dict[str, str], proto_config: dict
) -> tuple[str, dict]:
    """Pure end-to-end: Lua sources + proto_config JSON -> (proto text, rpc-map)."""
    modules = {name: parse_lua_module(name, src) for name, src in lua_sources.items()}
    resolve_types(modules)

    services_raw = proto_config.get("service", {})
    services: dict[str, dict] = {}
    for service_name, methods in services_raw.items():
        norm = {}
        for method, spec in methods.items():
            if isinstance(spec, dict) and "request" in spec and "response" in spec:
                norm[method] = {"request": spec["request"], "response": spec["response"]}
        if norm:
            services[service_name] = norm

    message_names = collect_message_names(modules)
    resolved_services, skipped = filter_services(services, message_names)
    if skipped:
        print(f"[extract] skipped {len(skipped)} debug/test RPCs with stripped "
              f"types: {', '.join(skipped)}")

    proto_text = render_proto(modules, resolved_services)
    rpc_map = build_rpc_map(resolved_services)

    for required in REQUIRED_MESSAGES:
        if required not in message_names:
            raise SystemExit(f"required message missing from extraction: {required}")

    return proto_text, rpc_map


# --------------------------------------------------------------------------- #
# Unity bundle download (IO)
# --------------------------------------------------------------------------- #
def _session() -> requests.Session:
    s = requests.Session()
    s.headers.update({"User-Agent": USER_AGENT})
    return s


def _get_bytes(s: requests.Session, url: str, timeout: int = 60) -> bytes:
    r = s.get(url, timeout=timeout)
    r.raise_for_status()
    return r.content


def _join(base: str, *parts: str) -> str:
    cur = base.rstrip("/") + "/"
    for p in parts:
        cur = urljoin(cur, p.lstrip("/"))
    return cur


def _origin(url: str) -> str:
    p = urlsplit(url)
    return f"{p.scheme}://{p.netloc}"


def _choose_url(entries: list[dict]) -> str:
    ordered = sorted(
        entries, key=lambda e: (e.get("Priority", 0), e.get("weight", 0)), reverse=True
    )
    return ordered[0]["url"]


def wanted_excel(path: str) -> bool:
    return bool(re.fullmatch(
        r"LuaByte/Lua/Excels/(?:Data/(?:amulet/amulet_(?:effect|badge)(?:_b\d+)?|str/event(?:_b\d+)?)"
        r"|Langs/str_event_(?:jp|chs|chs_t|en|kr))\.lua\.bytes", path
    ))


def fetch_from_unity(timeout: int = 60) -> tuple[dict[str, str], dict, dict[str, str], dict[str, str]]:
    """Download and decode the Protol Lua sources + proto_config.bytes.

    Returns (protocol Lua, service config, selected Excel Lua, metadata).
    """
    import UnityPy  # imported lazily so the pure logic is testable without it

    UnityPy.config.FALLBACK_UNITY_VERSION = UNITY_FALLBACK_VERSION
    warnings.filterwarnings("ignore", message="No valid Unity version found.*")

    s = _session()
    html = _get_bytes(s, CLIENT_URL, timeout).decode("utf-8", "replace")
    loader = re.search(r"Build/([^\"']+?\.loader\.js)", html)
    pv = re.search(r"productVersion\s*:\s*[\"']([^\"']+)[\"']", html)
    if not loader or "-WebGL-release-" not in loader.group(1):
        raise SystemExit("could not locate Unity WebGL loader in client HTML")
    issuer = loader.group(1).split("-WebGL-release-", 1)[0]
    product_version = pv.group(1) if pv else "unknown"
    print(f"[extract] issuer={issuer} productVersion={product_version}")

    cbs = json.loads(_get_bytes(
        s, _join(_origin(CLIENT_URL), f"assetbundles/clientBundleSettings/{issuer}-release.json"), timeout
    ))
    wh = cbs["warehouses"][0]
    whs = json.loads(_get_bytes(
        s, _join(_choose_url(wh["urls"]), wh["warehouseSettingPath"]), timeout
    ))
    bundle_base = _join(_choose_url(whs["urls"]), whs["bundlePath"]).rstrip("/") + "/"

    profile_base, bundle_hash = None, None
    for profile in TEXTURE_PROFILES:
        base = _join(bundle_base, profile) + "/"
        try:
            h = _get_bytes(s, _join(base, "bundle_hash.txt"), timeout).decode().strip()
        except requests.RequestException:
            continue
        if h:
            profile_base, bundle_hash = base, h
            break
    if not profile_base:
        raise SystemExit("no usable texture profile (ASTC/DXT)")
    print(f"[extract] profile_base={profile_base} bundle_hash={bundle_hash}")

    info = _get_bytes(s, _join(profile_base, "bundle_info_so.majset"), timeout)
    bundle_infos, asset_infos = _read_bundle_info(UnityPy, info)

    wanted: dict[int, list[str]] = {}
    for asset in asset_infos:
        path = asset.get("assetPath")
        idx = asset.get("ownerBundleIndex")
        if not isinstance(path, str) or idx is None:
            continue
        if path.startswith(PROTOL_PREFIX) and path.endswith(".lua.bytes"):
            wanted.setdefault(int(idx), []).append(path)
        elif path == PROTO_CONFIG_ASSET:
            wanted.setdefault(int(idx), []).append(path)
        elif wanted_excel(path):
            wanted.setdefault(int(idx), []).append(path)

    lua_sources: dict[str, str] = {}
    excel_sources: dict[str, str] = {}
    proto_config: dict | None = None
    for idx, paths in sorted(wanted.items()):
        name = bundle_infos[idx]["name"]
        assets = _extract_text_assets(UnityPy, _get_bytes(s, _join(profile_base, name), timeout))
        for path in paths:
            data = _lookup_asset(assets, path)
            if data is None:
                raise ValueError(f"missing TextAsset for {path} in {name}")
            if path == PROTO_CONFIG_ASSET:
                proto_config = json.loads(data.decode("utf-8", "replace"))
            elif path.startswith(PROTOL_PREFIX):
                module_name = posixpath.basename(path)[: -len(".lua.bytes")]
                lua_sources[module_name] = _decode_lua(data).decode("utf-8", "surrogateescape")
            else:
                excel_sources[path] = _decode_lua(data).decode("utf-8", "surrogateescape")

    if proto_config is None:
        raise SystemExit("proto_config.bytes not found in the Unity bundles")
    if not lua_sources:
        raise SystemExit("no Protol descriptor Lua found in the Unity bundles")

    meta = {"product_version": product_version, "bundle_hash": bundle_hash, "issuer": issuer}
    return lua_sources, proto_config, excel_sources, meta


def _read_bundle_info(unitypy, data: bytes) -> tuple[list[dict], list[dict]]:
    env = unitypy.load(data)
    for obj in env.objects:
        if obj.type.name != "MonoBehaviour":
            continue
        tree = obj.read_typetree()
        if "bundleInfos" in tree and "assetInfos" in tree:
            return tree["bundleInfos"], tree["assetInfos"]
    raise SystemExit("BundleInfoSO not found in bundle_info_so.majset")


def _extract_text_assets(unitypy, data: bytes) -> dict[str, bytes]:
    env = unitypy.load(data)
    out: dict[str, bytes] = {}
    for obj in env.objects:
        if obj.type.name != "TextAsset":
            continue
        ta = obj.read()
        name = getattr(ta, "m_Name", "")
        script = getattr(ta, "m_Script", "")
        if not name:
            continue
        out[name] = script if isinstance(script, bytes) else script.encode("utf-8", "surrogateescape")
    return out


def _lookup_asset(assets: dict[str, bytes], asset_path: str) -> bytes | None:
    base = posixpath.basename(asset_path)
    for candidate in (base, base[: -len(".bytes")] if base.endswith(".bytes") else base,
                      posixpath.splitext(base)[0]):
        if candidate in assets:
            return assets[candidate]
    return None


def _looks_like_lua(data: bytes) -> bool:
    sample = data[:4096]
    if not sample:
        return False
    printable = sum(b in (9, 10, 13) or 32 <= b <= 126 for b in sample)
    if printable / len(sample) < 0.85:
        return False
    return sample.lstrip().startswith((b"--", b"local", b"return", b"module", b"function"))


def _decode_lua(data: bytes) -> bytes:
    if _looks_like_lua(data):
        return data
    return bytes(b ^ LUA_XOR_KEY[i % len(LUA_XOR_KEY)] for i, b in enumerate(data))
