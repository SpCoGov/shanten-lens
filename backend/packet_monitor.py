from __future__ import annotations

import copy
import json
import threading
import time
from collections import deque
from typing import Any, Deque, Dict, List


def _json_safe(value: Any) -> Any:
    try:
        return json.loads(json.dumps(value, ensure_ascii=False, default=str))
    except Exception:
        return copy.deepcopy(value)


class PacketMonitor:
    def __init__(self, max_packets: int = 800):
        self.max_packets = max_packets
        self._lock = threading.Lock()
        self._seq = 0
        self._flow_seq = 0
        self._packets: Deque[Dict[str, Any]] = deque(maxlen=max_packets)
        self._flow_events: Deque[Dict[str, Any]] = deque(maxlen=max_packets)
        self._pending_group_by_msg_id: Dict[int, str] = {}

    def append(self, view: Dict[str, Any], flow: Dict[str, Any] | None = None) -> Dict[str, Any]:
        with self._lock:
            self._seq += 1
            seq = self._seq
            packet_type = str(view.get("type") or "")
            msg_id = view.get("id")
            method = str(view.get("method") or "")

            if packet_type == "Req" and isinstance(msg_id, int):
                group_key = f"req:{msg_id}:{seq}"
                self._pending_group_by_msg_id[msg_id] = group_key
            elif packet_type == "Res" and isinstance(msg_id, int):
                group_key = self._pending_group_by_msg_id.pop(msg_id, f"res:{msg_id}:{seq}")
            else:
                group_key = f"{packet_type.lower() or 'packet'}:{seq}"

            record = {
                "seq": seq,
                "ts": int(time.time() * 1000),
                "group_key": group_key,
                "msg_id": msg_id if isinstance(msg_id, int) else None,
                "packet_type": packet_type,
                "method": method,
                "from_client": bool(view.get("from_client")),
                "data": _json_safe(view.get("data")),
            }
            if flow:
                record["flow_id"] = flow.get("id")
                record["flow_peer_key"] = flow.get("peer_key")
                record["flow_client"] = flow.get("client")
                record["flow_server"] = flow.get("server")
            self._packets.append(record)
            return copy.deepcopy(record)

    def append_flow_event(self, event: str, flow: Dict[str, Any]) -> Dict[str, Any]:
        with self._lock:
            self._flow_seq += 1
            record = {
                "seq": self._flow_seq,
                "ts": int(time.time() * 1000),
                "event": event,
                "flow": _json_safe(flow),
            }
            self._flow_events.append(record)
            return copy.deepcopy(record)

    def snapshot(self) -> List[Dict[str, Any]]:
        with self._lock:
            return [copy.deepcopy(item) for item in self._packets]

    def flow_events_snapshot(self) -> List[Dict[str, Any]]:
        with self._lock:
            return [copy.deepcopy(item) for item in self._flow_events]

    def clear(self) -> None:
        with self._lock:
            self._packets.clear()
            self._flow_events.clear()
            self._pending_group_by_msg_id.clear()


PACKET_MONITOR = PacketMonitor()
