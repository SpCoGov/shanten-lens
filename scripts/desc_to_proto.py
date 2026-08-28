#!/usr/bin/env python3
"""Convert a binary FileDescriptorSet into one merged .proto file."""

from __future__ import annotations

import argparse
import os
import tempfile
from pathlib import Path

from google.protobuf import descriptor_pb2


TYPE_NAMES = {
    descriptor_pb2.FieldDescriptorProto.TYPE_DOUBLE: "double",
    descriptor_pb2.FieldDescriptorProto.TYPE_FLOAT: "float",
    descriptor_pb2.FieldDescriptorProto.TYPE_INT64: "int64",
    descriptor_pb2.FieldDescriptorProto.TYPE_UINT64: "uint64",
    descriptor_pb2.FieldDescriptorProto.TYPE_INT32: "int32",
    descriptor_pb2.FieldDescriptorProto.TYPE_FIXED64: "fixed64",
    descriptor_pb2.FieldDescriptorProto.TYPE_FIXED32: "fixed32",
    descriptor_pb2.FieldDescriptorProto.TYPE_BOOL: "bool",
    descriptor_pb2.FieldDescriptorProto.TYPE_STRING: "string",
    descriptor_pb2.FieldDescriptorProto.TYPE_BYTES: "bytes",
    descriptor_pb2.FieldDescriptorProto.TYPE_UINT32: "uint32",
    descriptor_pb2.FieldDescriptorProto.TYPE_SFIXED32: "sfixed32",
    descriptor_pb2.FieldDescriptorProto.TYPE_SFIXED64: "sfixed64",
    descriptor_pb2.FieldDescriptorProto.TYPE_SINT32: "sint32",
    descriptor_pb2.FieldDescriptorProto.TYPE_SINT64: "sint64",
}


def ind(level: int, text: str) -> str:
    return "  " * level + text


def field_type(field, messages: dict[str, object]) -> str:
    if field.type in (field.TYPE_MESSAGE, field.TYPE_ENUM, field.TYPE_GROUP):
        return field.type_name
    return TYPE_NAMES[field.type]


def option_suffix(field) -> str:
    options: list[str] = []
    if field.HasField("default_value"):
        value = field.default_value
        if field.type in (field.TYPE_STRING, field.TYPE_BYTES):
            value = repr(value).replace("'", '"')
        options.append(f"default = {value}")
    if field.options.HasField("packed"):
        options.append(f"packed = {str(field.options.packed).lower()}")
    if field.options.deprecated:
        options.append("deprecated = true")
    return f" [{', '.join(options)}]" if options else ""


def render_enum(enum, level: int) -> list[str]:
    lines = [ind(level, f"enum {enum.name} {{")]
    if enum.options.allow_alias:
        lines.append(ind(level + 1, "option allow_alias = true;"))
    for value in enum.value:
        suffix = " [deprecated = true]" if value.options.deprecated else ""
        lines.append(ind(level + 1, f"{value.name} = {value.number}{suffix};"))
    if enum.reserved_range:
        ranges = []
        for item in enum.reserved_range:
            ranges.append(str(item.start) if item.start == item.end else f"{item.start} to {item.end}")
        lines.append(ind(level + 1, f"reserved {', '.join(ranges)};"))
    if enum.reserved_name:
        names = ", ".join(f'"{name}"' for name in enum.reserved_name)
        lines.append(ind(level + 1, f"reserved {names};"))
    lines.append(ind(level, "}"))
    return lines


def render_message(message, full_name: str, messages: dict[str, object], syntax: str, level: int) -> list[str]:
    lines = [ind(level, f"message {message.name} {{")]
    map_entries = {
        f"{full_name}.{nested.name}": nested
        for nested in message.nested_type
        if nested.options.map_entry
    }

    for enum in message.enum_type:
        lines.extend(render_enum(enum, level + 1))
        lines.append("")
    for nested in message.nested_type:
        if not nested.options.map_entry:
            lines.extend(render_message(nested, f"{full_name}.{nested.name}", messages, syntax, level + 1))
            lines.append("")

    synthetic_oneofs = {
        field.oneof_index
        for field in message.field
        if field.proto3_optional and field.HasField("oneof_index")
    }
    oneof_fields: dict[int, list[object]] = {}
    normal_fields = []
    for field in message.field:
        if field.HasField("oneof_index") and field.oneof_index not in synthetic_oneofs:
            oneof_fields.setdefault(field.oneof_index, []).append(field)
        else:
            normal_fields.append(field)

    def render_field(field, oneof: bool = False) -> str:
        target = map_entries.get(field.type_name)
        if target is not None:
            key, value = target.field
            type_name = f"map<{field_type(key, messages)}, {field_type(value, messages)}>"
            label = ""
        else:
            type_name = field_type(field, messages)
            if oneof:
                label = ""
            elif field.proto3_optional:
                label = "optional "
            elif field.label == field.LABEL_REPEATED:
                label = "repeated "
            elif syntax != "proto3" and field.label == field.LABEL_REQUIRED:
                label = "required "
            elif syntax != "proto3" and field.label == field.LABEL_OPTIONAL:
                label = "optional "
            else:
                label = ""
        return f"{label}{type_name} {field.name} = {field.number}{option_suffix(field)};"

    for field in normal_fields:
        lines.append(ind(level + 1, render_field(field)))
    for index, fields in sorted(oneof_fields.items()):
        lines.append(ind(level + 1, f"oneof {message.oneof_decl[index].name} {{"))
        for field in fields:
            lines.append(ind(level + 2, render_field(field, oneof=True)))
        lines.append(ind(level + 1, "}"))

    if message.extension_range:
        ranges = []
        for item in message.extension_range:
            end = "max" if item.end == 536_870_912 else str(item.end - 1)
            ranges.append(str(item.start) if end == str(item.start) else f"{item.start} to {end}")
        lines.append(ind(level + 1, f"extensions {', '.join(ranges)};"))
    if message.reserved_range:
        ranges = []
        for item in message.reserved_range:
            end = item.end - 1
            ranges.append(str(item.start) if item.start == end else f"{item.start} to {end}")
        lines.append(ind(level + 1, f"reserved {', '.join(ranges)};"))
    if message.reserved_name:
        names = ", ".join(f'"{name}"' for name in message.reserved_name)
        lines.append(ind(level + 1, f"reserved {names};"))
    if message.options.deprecated:
        lines.insert(1, ind(level + 1, "option deprecated = true;"))

    while len(lines) > 1 and lines[-1] == "":
        lines.pop()
    lines.append(ind(level, "}"))
    return lines


def render_service(service, level: int = 0) -> list[str]:
    lines = [ind(level, f"service {service.name} {{")]
    if service.options.deprecated:
        lines.append(ind(level + 1, "option deprecated = true;"))
    for method in service.method:
        request = ("stream " if method.client_streaming else "") + method.input_type
        response = ("stream " if method.server_streaming else "") + method.output_type
        if method.options.deprecated:
            lines.append(ind(level + 1, f"rpc {method.name} ({request}) returns ({response}) {{"))
            lines.append(ind(level + 2, "option deprecated = true;"))
            lines.append(ind(level + 1, "}"))
        else:
            lines.append(ind(level + 1, f"rpc {method.name} ({request}) returns ({response});"))
    lines.append(ind(level, "}"))
    return lines


def convert(source: Path, package: str) -> str:
    descriptor_set = descriptor_pb2.FileDescriptorSet()
    descriptor_set.ParseFromString(source.read_bytes())
    if not descriptor_set.file:
        raise ValueError("descriptor set contains no files")

    files = [file for file in descriptor_set.file if file.package == package]
    if not files:
        available = sorted({file.package for file in descriptor_set.file})
        raise ValueError(f"package {package!r} not found; available packages: {available}")
    syntaxes = {file.syntax or "proto2" for file in files}
    if len(syntaxes) != 1:
        raise ValueError(f"cannot merge syntaxes: {syntaxes}")
    syntax = next(iter(syntaxes))
    included = {file.name for file in files}
    imports = sorted({dep for file in files for dep in file.dependency if dep not in included})

    messages: dict[str, object] = {}

    def index_message(message, prefix: str) -> None:
        name = f"{prefix}.{message.name}"
        messages[name] = message
        for nested in message.nested_type:
            index_message(nested, name)

    for file in files:
        prefix = f".{file.package}" if file.package else ""
        for message in file.message_type:
            index_message(message, prefix)

    lines = [f'syntax = "{syntax}";', ""]
    if package:
        lines.extend([f"package {package};", ""])
    for dependency in imports:
        lines.append(f'import "{dependency}";')
    if imports:
        lines.append("")

    for file in files:
        prefix = f".{file.package}" if file.package else ""
        for enum in file.enum_type:
            lines.extend(render_enum(enum, 0))
            lines.append("")
        for message in file.message_type:
            lines.extend(render_message(message, f"{prefix}.{message.name}", messages, syntax, 0))
            lines.append("")
        for service in file.service:
            lines.extend(render_service(service))
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=Path(r"C:\Users\Administrator\Downloads\liqi.desc"))
    parser.add_argument("output", nargs="?", type=Path, default=Path(r"D:\shanten-lens\proto\liqi.proto"))
    parser.add_argument("--package", default="lq", help="package to extract (default: lq)")
    args = parser.parse_args()

    text = convert(args.source, args.package)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile("w", encoding="utf-8", newline="\n", dir=args.output.parent, delete=False) as tmp:
        tmp.write(text)
        temporary = tmp.name
    os.replace(temporary, args.output)
    print(f"wrote {args.output} ({len(text.encode('utf-8'))} bytes)")


if __name__ == "__main__":
    main()
