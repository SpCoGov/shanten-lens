"""Read Unity TextAssets and turn their Lua descriptors into a validated schema."""
from __future__ import annotations

import ast
import json
import re
import warnings
from pathlib import PurePosixPath
from urllib.error import HTTPError
from urllib.parse import urljoin
from urllib.request import Request, urlopen

from google.protobuf import descriptor_pb2 as pb, descriptor_pool

CLIENT_URL = "https://game.maj-soul.com/1/"

_QUOTED = r'''"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*' '''.strip()
_VALUE = rf"(?:{_QUOTED}|\{{[^{{}}]*\}}|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)?)"
_LUA_DATA = re.compile(
    rf"(?P<comment>--[^\r\n]*)|"
    rf"(?P<import>local\s+(?P<alias>\w+)\s*=\s*require\s*\(?\s*(?P<path>{_QUOTED}))|"
    r"(?P<declaration>(?P<variable>\w+)\s*=\s*\w+\.(?P<kind>Descriptor|FieldDescriptor|EnumDescriptor|EnumValueDescriptor)\s*\(\s*\))|"
    rf"(?P<assignment>(?P<owner>\w+)\.(?P<attribute>\w+)\s*=\s*(?P<value>{_VALUE}))|"
    rf"(?P<string>{_QUOTED})"
)
_ATTRIBUTES = {"name", "full_name", "fields", "nested_types", "enum_types", "values",
               "number", "label", "type", "message_type", "enum_type", "containing_type",
               "extensions", "is_extendable", "has_default_value", "default_value", "index", "cpp_type"}


def _literal(value: str):
    if value.startswith(("'", '"')):
        return ast.literal_eval(value)
    if value.startswith("{"):
        elements = value[1:-1].strip()
        return [item.strip() for item in elements.split(",")] if elements else []
    if value in ("true", "false", "nil"):
        return {"true": True, "false": False, "nil": None}[value]
    if re.fullmatch(r"-?\d+", value):
        return int(value)
    return float(value) if re.fullmatch(r"-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?", value) else value


def read_protocol(sources: dict[str, str], config: dict) -> pb.FileDescriptorProto:
    nodes, imports = {}, {}
    for module, source in sorted(sources.items()):
        imports[module] = {}
        properties = []
        for match in _LUA_DATA.finditer(source):
            if match["import"]:
                imports[module][match["alias"]] = _literal(match["path"]).rsplit(".", 1)[-1]
            elif match["declaration"]:
                key = (module, match["variable"])
                if key in nodes:
                    raise ValueError(f"duplicate descriptor: {key}")
                nodes[key] = {"kind": match["kind"]}
            elif match["assignment"]:
                properties.append((match["owner"], match["attribute"], match["value"]))
        for owner, attribute, value in properties:
            node = nodes.get((module, owner))
            if node is None:
                continue
            if attribute not in _ATTRIBUTES or attribute in node:
                raise ValueError(f"unsupported or duplicate descriptor property: {owner}.{attribute}")
            node[attribute] = _literal(value)

    def reference(module, value, kind):
        if not isinstance(value, str) or not re.fullmatch(r"\w+(?:\.\w+)?", value):
            raise ValueError(f"invalid descriptor reference: {value}")
        if "." in value:
            alias, variable = value.split(".")
            module = imports[module].get(alias)
        else:
            variable = value
        node = nodes.get((module, variable))
        if node is None or node["kind"] != kind:
            raise ValueError(f"unresolved {kind}: {module}/{value}")
        return node

    types = {}
    for (module, variable), node in nodes.items():
        if node["kind"] not in ("Descriptor", "EnumDescriptor"):
            continue
        full_name = node.get("full_name", "")
        if not full_name.startswith(".lq.") or full_name.startswith((".lq.config.", ".lq.lqc.")):
            continue
        if not re.fullmatch(r"\.lq(?:\.[A-Za-z_]\w*)+", full_name) or node.get("name") != full_name.rsplit(".", 1)[-1]:
            raise ValueError(f"invalid type name: {full_name}")
        if full_name in types:
            raise ValueError(f"duplicate protocol type: {full_name}")
        types[full_name] = (module, node)
    if not types:
        raise ValueError("no lq protocol descriptors")

    schema = pb.FileDescriptorProto(name="liqi.proto", package="lq", syntax="proto3")
    messages = {}
    children = {}
    for full_name, (_, node) in types.items():
        children.setdefault((full_name.rsplit(".", 1)[0], node["kind"]), set()).add(full_name)
    for full_name, (module, node) in sorted(types.items()):
        parent = full_name.rsplit(".", 1)[0]
        container = schema if parent == ".lq" else messages.get(parent)
        if container is None:
            raise ValueError(f"missing containing message: {full_name}")
        if node.get("containing_type") is not None:
            declared_parent = reference(module, node["containing_type"], "Descriptor")
            if declared_parent.get("full_name") != parent:
                raise ValueError(f"incorrect containing message: {full_name}")
        if node["kind"] == "Descriptor":
            target = container.message_type.add() if parent == ".lq" else container.nested_type.add()
            target.name = node["name"]
            messages[full_name] = target
        else:
            target = container.enum_type.add(name=node["name"])
            for value in node["values"]:
                entry = reference(module, value, "EnumValueDescriptor")
                if type(entry["number"]) is not int:
                    raise ValueError(f"invalid enum number: {full_name}")
                target.value.add(name=entry["name"], number=entry["number"])
            if len({value.number for value in target.value}) != len(target.value):
                target.options.allow_alias = True

    for full_name, target in messages.items():
        module, node = types[full_name]
        if node.get("is_extendable") or node.get("extensions"):
            raise ValueError(f"proto3 extensions are unsupported: {full_name}")
        for attribute, kind in (("nested_types", "Descriptor"), ("enum_types", "EnumDescriptor")):
            linked = [reference(module, value, kind).get("full_name") for value in node.get(attribute, [])]
            if len(set(linked)) != len(linked) or set(linked) != children.get((full_name, kind), set()):
                raise ValueError(f"incorrect {attribute}: {full_name}")
        for value in node["fields"]:
            field = reference(module, value, "FieldDescriptor")
            if field.get("has_default_value"):
                raise ValueError(f"proto3 explicit defaults are unsupported: {full_name}")
            if any(type(field[key]) is not int for key in ("number", "label", "type")):
                raise ValueError(f"invalid field number, label or type: {full_name}.{field['name']}")
            output = target.field.add(name=field["name"], number=field["number"], label=field["label"], type=field["type"])
            if output.type in (pb.FieldDescriptorProto.TYPE_MESSAGE, pb.FieldDescriptorProto.TYPE_ENUM):
                kind = "Descriptor" if output.type == pb.FieldDescriptorProto.TYPE_MESSAGE else "EnumDescriptor"
                key = "message_type" if output.type == pb.FieldDescriptorProto.TYPE_MESSAGE else "enum_type"
                output.type_name = reference(module, field.get(key), kind).get("full_name", "")

    omitted = []
    for service_name, methods in sorted(config.get("service", {}).items()):
        service = pb.ServiceDescriptorProto(name=service_name)
        for method_name, method in sorted(methods.items()):
            input_type, output_type = ".lq." + method["request"], ".lq." + method["response"]
            if input_type not in messages or output_type not in messages:
                omitted.append(f"{service_name}.{method_name}")
                continue
            service.method.add(name=method_name, input_type=input_type, output_type=output_type)
        if service.method:
            schema.service.add().CopyFrom(service)
    if omitted:
        print(f"[protocol] omitted {len(omitted)} RPCs without shipped message descriptors: {', '.join(omitted)}")
    try:
        descriptor_pool.DescriptorPool().Add(schema)
    except (TypeError, ValueError) as error:
        raise ValueError(f"invalid protocol descriptors: {error}") from error
    return schema


def render_schema(schema: pb.FileDescriptorProto) -> str:
    lines = ['syntax = "proto3";', "", "package lq;", ""]

    def enum(item, indent):
        lines.append(indent + f"enum {item.name} {{")
        if item.options.allow_alias:
            lines.append(indent + "    option allow_alias = true;")
        for value in item.value:
            lines.append(indent + f"    {value.name} = {value.number};")
        lines.append(indent + "}")

    def message(item, indent):
        lines.append(indent + f"message {item.name} {{")
        for child in sorted(item.enum_type, key=lambda child: child.name):
            enum(child, indent + "    ")
        for child in sorted(item.nested_type, key=lambda child: child.name):
            message(child, indent + "    ")
        for field in sorted(item.field, key=lambda field: field.number):
            kind = field.type_name or pb.FieldDescriptorProto.Type.Name(field.type).removeprefix("TYPE_").lower()
            label = "repeated " if field.label == pb.FieldDescriptorProto.LABEL_REPEATED else ""
            lines.append(indent + f"    {label}{kind} {field.name} = {field.number};")
        lines.append(indent + "}")

    for item in sorted(schema.enum_type, key=lambda item: item.name):
        enum(item, "")
        lines.append("")
    for item in sorted(schema.message_type, key=lambda item: item.name):
        message(item, "")
        lines.append("")
    for service in schema.service:
        lines.append(f"service {service.name} {{")
        for method in service.method:
            lines.append(f"    rpc {method.name} ({method.input_type.removeprefix('.lq.')}) returns ({method.output_type.removeprefix('.lq.')});")
        lines.extend(["}", ""])
    return "\n".join(lines).rstrip() + "\n"


def build_outputs(lua_sources: dict[str, str], proto_config: dict) -> tuple[str, dict]:
    schema = read_protocol(lua_sources, proto_config)
    required = {"Wrapper", "ActionPrototype", "RecordNewRound", "OptionalOperationList",
                "ResAuthGame", "ResEnterGame", "ReqSelfOperation", "ResSyncGame", "ReqChiPengGang", "GameEndResult"}
    missing = required - {message.name for message in schema.message_type}
    if missing:
        raise ValueError(f"required protocol messages missing: {sorted(missing)}")
    rpc = {f".lq.{service.name}.{method.name}": {"req": method.input_type, "resp": method.output_type}
           for service in schema.service for method in service.method}
    return render_schema(schema), rpc


def wanted_excel(path: str) -> bool:
    relative = path.removeprefix("LuaByte/Lua/Excels/")
    return path != relative and bool(re.fullmatch(
        r"Data/(?:amulet/amulet_(?:effect|badge|activity|level|node|enemy|character|shop_upgrade|forge|transport|trade_item|gamble_selection)|str/event)(?:_b\d+)?\.lua\.bytes|"
        r"Langs/str_event_(?:chs|chs_t|jp|en|kr)\.lua\.bytes", relative))


def decode_lua(data: bytes) -> str:
    key = b"wrelupqezdfrqdsd"
    for payload in (data, bytes(byte ^ key[index % len(key)] for index, byte in enumerate(data))):
        try:
            text = payload.decode("utf-8-sig")
        except UnicodeDecodeError:
            continue
        if re.match(r"\s*(?:--|local\b|return\b|module\s*\()", text):
            return text
    raise ValueError("TextAsset is neither plain nor XOR-encoded Lua")


def fetch_from_unity(timeout: int = 60) -> tuple[dict[str, str], dict, dict[str, str], dict[str, str]]:
    import UnityPy

    UnityPy.config.FALLBACK_UNITY_VERSION = "2022.3.62f2c1"
    warnings.filterwarnings("ignore", message="No valid Unity version found.*")

    def download(url):
        with urlopen(Request(url, headers={"User-Agent": "shanten-lens-data-updater"}), timeout=timeout) as response:
            return response.read()

    def address(entries, path):
        preferred = max(entries, key=lambda entry: (entry.get("Priority", 0), entry.get("weight", 0)))
        return urljoin(preferred["url"].rstrip("/") + "/", path)

    html = download(CLIENT_URL).decode("utf-8")
    loader = re.search(r"([\w]+)-WebGL-release-[^\"'\s]+\.loader\.js", html)
    version = re.search(r"productVersion\s*:\s*[\"']([^\"']+)", html)
    if loader is None or version is None:
        raise ValueError("client HTML has no Unity loader or product version")
    issuer = loader[1]
    settings = json.loads(download(urljoin(CLIENT_URL, f"/assetbundles/clientBundleSettings/{issuer}-release.json")))
    warehouse = settings["warehouses"][0]
    settings = json.loads(download(address(warehouse["urls"], warehouse["warehouseSettingPath"])))
    base = address(settings["urls"], settings["bundlePath"]).rstrip("/") + "/"
    for profile in ("ASTC", "DXT"):
        bundle_base = urljoin(base, profile + "/")
        try:
            bundle_hash = download(urljoin(bundle_base, "bundle_hash.txt")).decode("ascii").strip()
            manifest = UnityPy.load(download(urljoin(bundle_base, "bundle_info_so.majset")))
            break
        except HTTPError as error:
            if error.code != 404:
                raise
    else:
        raise ValueError("no supported Unity texture profile")
    if not re.fullmatch(r"[0-9a-fA-F]{32,64}", bundle_hash):
        raise ValueError("invalid Unity bundle hash")
    indexes = [obj.read_typetree() for obj in manifest.objects if obj.type.name == "MonoBehaviour"]
    indexes = [item for item in indexes if item.get("m_Name") == "BundleInfoSO"]
    if len(indexes) != 1:
        raise ValueError("expected one BundleInfoSO resource index")
    index = indexes[0]
    selected = {}
    for asset in index["assetInfos"]:
        path = asset["assetPath"]
        if re.fullmatch(r"LuaByte/Lua/Protol/\w+_pb\.lua\.bytes", path) or wanted_excel(path) or path == "MyAssets/docs/proto_config.bytes":
            if path in selected:
                raise ValueError(f"duplicate resource address: {path}")
            owner = asset["ownerBundleIndex"]
            if type(owner) is not int or not 0 <= owner < len(index["bundleInfos"]):
                raise ValueError(f"invalid resource bundle index: {path}")
            selected[path] = owner
    assets = {}
    for owner in sorted(set(selected.values())):
        bundle = UnityPy.load(download(urljoin(bundle_base, index["bundleInfos"][owner]["name"])))
        container = dict(bundle.container.items())
        for path, asset_owner in selected.items():
            if asset_owner != owner:
                continue
            reader = container.get(path.lower())
            if reader is None or reader.type.name != "TextAsset":
                raise ValueError(f"missing TextAsset: {path}")
            payload = reader.read().m_Script
            assets[path] = payload.encode("utf-8", "surrogateescape") if isinstance(payload, str) else bytes(payload)
    lua = {PurePosixPath(path).name.removesuffix(".lua.bytes"): decode_lua(payload)
           for path, payload in assets.items() if path.startswith("LuaByte/Lua/Protol/")}
    excels = {path: decode_lua(payload) for path, payload in assets.items() if wanted_excel(path)}
    config = json.loads(assets["MyAssets/docs/proto_config.bytes"])
    meta = {"product_version": version[1], "bundle_hash": bundle_hash, "issuer": issuer}
    print(f"[client] {meta}; protocol modules={len(lua)}, Excel assets={len(excels)}")
    return lua, config, excels, meta
