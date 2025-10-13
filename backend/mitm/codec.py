import base64, json, os, struct
from enum import Enum
from typing import List, Dict, Tuple, Any
from google.protobuf.json_format import MessageToDict, ParseDict

from proto import liqi_pb2 as pb

_DEFAULT_LIQI_JSON = os.path.abspath(os.path.join(os.path.dirname(__file__), "../../proto/liqi.json"))
_KEYS = [0x84, 0x5e, 0x4e, 0x42, 0x39, 0xa2, 0x1f, 0x60, 0x1c]


def _xor(data: bytes) -> bytes:
    b = bytearray(data)
    n = len(b)
    for i in range(n):
        u = (23 ^ n) + 5 * i + _KEYS[i % len(_KEYS)] & 255
        b[i] ^= u
    return bytes(b)


def _to_varint(x: int) -> bytes:
    if x == 0: return b"\x00"
    out = bytearray()
    while x > 0:
        b = x & 0x7F
        x >>= 7
        if x: b |= 0x80
        out.append(b)
    return bytes(out)


def _parse_varint(buf, p):
    x = 0
    s = 0
    while p < len(buf):
        b = buf[p]
        p += 1
        x |= (b & 0x7F) << s
        if not (b & 0x80): break
        s += 7
    return x, p


def _from_protobuf(buf: bytes) -> List[Dict]:
    p = 0
    out = []
    while p < len(buf):
        tag = buf[p]
        p += 1
        typ = tag & 7
        fid = tag >> 3
        if typ == 0:
            val, p = _parse_varint(buf, p)
            out.append({"id": fid, "type": "varint", "data": val})
        elif typ == 2:
            ln, p = _parse_varint(buf, p)
            data = buf[p:p + ln]
            p += ln
            out.append({"id": fid, "type": "string", "data": data})
        else:
            raise ValueError(f"wire={typ}")
    return out


def _to_protobuf(fields):
    out = bytearray()
    for f in fields:
        if f["type"] == "varint":
            out += _to_varint((f["id"] << 3) | 0)
            out += _to_varint(f["data"])
        elif f["type"] == "string":
            out += _to_varint((f["id"] << 3) | 2)
            out += _to_varint(len(f["data"]))
            out += f["data"]
    return bytes(out)


class MsgType(Enum):
    Notify = 1
    Req = 2
    Res = 3


class LiqiCodec:
    def __init__(self, liqi_json_path=None):
        self.liqi_json_path = liqi_json_path or _DEFAULT_LIQI_JSON
        self.jsonProto = json.load(open(self.liqi_json_path, "r", encoding="utf-8"))
        self._res_map = {}
        self._last_req_id = 1

    def parse_frame(self, content: bytes, from_client: bool) -> Dict[str, Any]:
        if not content: raise ValueError("empty")
        mt = MsgType(content[0])
        if mt == MsgType.Notify:
            msg_id = None
            envelope = content[1:]
        else:
            msg_id = struct.unpack("<H", content[1:3])[0]
            envelope = content[3:]
        msg_block = _from_protobuf(envelope)
        method = msg_block[0]["data"].decode(errors="ignore")
        payload = msg_block[1]["data"]

        if mt == MsgType.Notify:
            data = self._decode_notify(method, payload)
        elif mt == MsgType.Req:
            method, data = self._decode_req(msg_id, method, payload)
            self._last_req_id = msg_id
        else:
            method, data = self._decode_res(msg_id, method, payload)

        return {"id": msg_id, "type": mt.name, "method": method, "data": data,
                "from_client": from_client, "raw": content}

    def build_frame(self, view: Dict[str, Any]) -> bytes:
        t = view["type"]
        method = view["method"]
        data = view["data"]
        if t == "Notify": return self._compose_notify(method, data)
        msg_id = view.get("id", (self._last_req_id - 8) % 256)
        if t == "Req": return self._compose_reqres("Req", method, data, msg_id)
        if t == "Res": return self._compose_reqres("Res", method, data, msg_id)
        raise ValueError("unknown type")

    def _decode_notify(self, method: str, payload: bytes) -> dict:
        name = method.split(".")[-1]
        if hasattr(pb, name):
            obj = getattr(pb, name).FromString(payload)
            d = MessageToDict(obj, always_print_fields_with_no_presence=True)
            if "data" in d:
                raw = base64.b64decode(d["data"])
                inner = d.get("name")
                if inner and hasattr(pb, inner):
                    inner_obj = getattr(pb, inner).FromString(_xor(raw))
                    d["data"] = MessageToDict(inner_obj, always_print_fields_with_no_presence=True)
            return d
        return {"_raw": base64.b64encode(payload).decode()}

    def _decode_req(self, msg_id: int, method: str, payload: bytes):
        lq, svc, rpc = self._split(method)
        dom = self.jsonProto["nested"][lq]["nested"][svc]["methods"][rpc]
        req_t, resp_t = dom["requestType"], dom["responseType"]
        req_cls, resp_cls = getattr(pb, req_t), getattr(pb, resp_t)
        obj = req_cls.FromString(payload)
        d = MessageToDict(obj, always_print_fields_with_no_presence=True)
        self._res_map[msg_id] = (method, resp_cls)
        return method, d

    def _decode_res(self, msg_id: int, method: str, payload: bytes):
        if msg_id not in self._res_map:
            return method or "(unknown_res)", {"_raw": base64.b64encode(payload).decode()}
        m, cls = self._res_map.pop(msg_id)
        obj = cls.FromString(payload)
        d = MessageToDict(obj, always_print_fields_with_no_presence=True)
        return m, d

    # === encode ===
    def _compose_reqres(self, t: str, method: str, data: dict, msg_id: int) -> bytes:
        lq, svc, rpc = self._split(method)
        dom = self.jsonProto["nested"][lq]["nested"][svc]["methods"][rpc]
        name = dom["requestType"] if t == "Req" else dom["responseType"]
        obj = ParseDict(data, getattr(pb, name)())
        blk = [{"id": 1, "type": "string", "data": method.encode()},
               {"id": 2, "type": "string", "data": obj.SerializeToString()}]
        head = b"\x02" if t == "Req" else b"\x03"
        return head + struct.pack("<H", msg_id) + _to_protobuf(blk)

    def _compose_notify(self, method: str, data: dict) -> bytes:
        name = method.split(".")[-1]
        if "data" in data and "name" in data:
            inner = data["name"]
            if hasattr(pb, inner):
                inner_obj = ParseDict(data["data"], getattr(pb, inner)())
                raw = inner_obj.SerializeToString()
                data["data"] = base64.b64encode(_xor(raw))
        if hasattr(pb, name):
            outer = ParseDict(data, getattr(pb, name)())
            payload = outer.SerializeToString()
        else:
            payload = base64.b64decode(data.get("_raw", b""))
        blk = [{"id": 1, "type": "string", "data": method.encode()},
               {"id": 2, "type": "string", "data": payload}]
        return b"\x01" + _to_protobuf(blk)

    @staticmethod
    def _split(method: str) -> Tuple[str, str, str]:
        if "/" in method:
            parts = method.split(".")
            lq = parts[-2] if len(parts) >= 2 else "lq"
            svc, rpc = parts[-1].split("/", 1)
        else:
            parts = method.split(".")
            lq = parts[-3] if len(parts) >= 3 else "lq"
            svc = parts[-2] if len(parts) >= 2 else ""
            rpc = parts[-1] if len(parts) >= 1 else ""
        return lq, svc, rpc
