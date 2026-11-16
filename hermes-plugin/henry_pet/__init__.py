"""henry_pet bridge: Hermes agent activity -> Henry's mood (PLAN #15, #19).

Only live conversations count: desktop / Telegram / CLI turns of the default profile. Cron jobs,
kanban workers and subagents never reach her. Fire-and-forget: a background thread posts to
``http://127.0.0.1:47821/mood`` (override with ``HENRY_PET_URL``); if Henry isn't running the
post fails silently and Hermes never waits on it.
"""

from __future__ import annotations

import json
import logging
import os
import queue
import threading
import urllib.request

logger = logging.getLogger(__name__)

URL = os.environ.get("HENRY_PET_URL", "http://127.0.0.1:47821").rstrip("/")
LIVE_PLATFORMS = {"desktop", "telegram", "cli", "tui", "gui", "local"}
PROFILE = os.environ.get("HENRY_PET_PROFILE", "default")

_live: dict[str, str] = {}
_lock = threading.Lock()
_q: "queue.Queue[dict]" = queue.Queue(maxsize=256)
_worker_started = False


def _worker() -> None:
    while True:
        body = _q.get()
        try:
            req = urllib.request.Request(
                URL + "/mood",
                data=json.dumps(body).encode(),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            urllib.request.urlopen(req, timeout=0.5).read()
        except Exception:  # Henry not running / busy: moods are best-effort
            pass


def _ensure_worker() -> None:
    global _worker_started
    if not _worker_started:
        _worker_started = True
        threading.Thread(target=_worker, name="henry-pet-bridge", daemon=True).start()


def _profile_ok() -> bool:
    try:
        from hermes_cli.profiles import get_active_profile_name

        return (get_active_profile_name() or "default") == PROFILE
    except Exception:
        return True


def _send(session_id, state: str, ttl: float = 600) -> None:
    if not session_id:
        return
    with _lock:
        if session_id not in _live:
            return
    try:
        _q.put_nowait({"source": f"hermes:{session_id}", "state": state, "ttl_s": ttl})
    except queue.Full:
        pass


def _on_pre_llm_call(session_id=None, platform=None, parent_session_id=None, **_):
    if not session_id or parent_session_id:
        return None
    if str(platform or "").lower() not in LIVE_PLATFORMS or not _profile_ok():
        return None
    with _lock:
        _live[session_id] = str(platform)
        if len(_live) > 500:
            _live.pop(next(iter(_live)))
    _send(session_id, "review")
    return None


def _on_stream_start(session_id=None, **_):
    _send(session_id, "review")


def _on_pre_tool_call(session_id=None, tool_name=None, **_):
    _send(session_id, "run", ttl=900)
    return None


def _todos_done(result) -> bool:
    try:
        data = json.loads(result) if isinstance(result, str) else result
        todos = data.get("todos") if isinstance(data, dict) else None
        return bool(todos) and all(t.get("status") in ("completed", "cancelled") for t in todos)
    except Exception:
        return False


def _on_post_tool_call(session_id=None, tool_name=None, result=None, **_):
    if tool_name in ("todo", "todo_list") and _todos_done(result):
        _send(session_id, "jump")
    _send(session_id, "review")


def _on_human_input_request(session_id=None, **_):
    _send(session_id, "waiting", ttl=1800)


def _on_human_input_resolved(session_id=None, **_):
    _send(session_id, "run")


def _on_session_end(session_id=None, completed=None, failed=None, interrupted=None, **_):
    if failed:
        _send(session_id, "failed")
    elif completed:
        _send(session_id, "wave")
    else:
        _send(session_id, "idle")


def register(ctx) -> None:
    if os.environ.get("HERMES_KANBAN_TASK"):
        return  # kanban workers never drive her
    _ensure_worker()
    ctx.register_hook("pre_llm_call", _on_pre_llm_call)
    ctx.register_hook("on_stream_start", _on_stream_start)
    ctx.register_hook("pre_tool_call", _on_pre_tool_call)
    ctx.register_hook("post_tool_call", _on_post_tool_call)
    ctx.register_hook("on_human_input_request", _on_human_input_request)
    ctx.register_hook("on_human_input_resolved", _on_human_input_resolved)
    ctx.register_hook("on_session_end", _on_session_end)
