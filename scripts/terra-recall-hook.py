#!/usr/bin/env python3
"""terra recall hook — UserPromptSubmit.

Reads the submitted prompt, asks terra for entities relevant by semantic
similarity and by recent touch, and injects a short, capped list of
slug+description *pointers* as additionalContext. The agent recalls full
detail on demand — the hook never injects payloads, only signposts.

Fail-open: any error (terra down, bad config, bad input) results in NO
injection, never a broken prompt.

Tunable via recall-hook.json beside this script (or $TERRA_HOOK_CONFIG).
"""

import json
import os
import sys
import urllib.request

DEFAULTS = {
    "url": "http://127.0.0.1:7373/query",
    "branch": "main",
    "similar_limit": 5,      # semantic matches to consider
    "touched_limit": 4,      # recently-touched entities to consider
    "min_similarity": 0.35,  # gate: drop weak semantic matches (noise on a small store)
    "max_total": 6,          # hard cap on injected pointers
    "with_description": True, # include the one-line description per slug
    "request_timeout": 5,    # per-request seconds; 2 requests must stay under the hook timeout
    "enabled": True,
}


def load_config():
    cfg = dict(DEFAULTS)
    path = os.environ.get("TERRA_HOOK_CONFIG") or os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "recall-hook.json"
    )
    try:
        with open(path) as f:
            cfg.update(json.load(f))
    except Exception:
        pass  # missing/bad config → defaults; never break the prompt
    cfg["url"] = os.environ.get("TERRA_URL", cfg["url"])
    return cfg


def query(url, body, timeout):
    req = urllib.request.Request(
        url, data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)


def collect(cfg):
    seen, items = set(), []
    timeout = cfg.get("request_timeout", 5)

    def add(res):
        if isinstance(res, list):
            for e in res:
                slug = e.get("slug") if isinstance(e, dict) else None
                if slug and slug not in seen:
                    seen.add(slug)
                    items.append(e)

    try:
        add(query(cfg["url"], {
            "command": "entities.similar", "branch": cfg["branch"],
            "queries": [cfg["_prompt"]], "limit": cfg["similar_limit"],
            "min_similarity": cfg["min_similarity"],
        }, timeout))
    except Exception:
        pass
    try:
        add(query(cfg["url"], {
            "command": "entities.touched", "branch": cfg["branch"], "limit": cfg["touched_limit"],
        }, timeout))
    except Exception:
        pass

    return items[: cfg["max_total"]]


def describe(e):
    d = e.get("description")
    if isinstance(d, str):
        return d
    return json.dumps(d) if d is not None else ""


def main():
    cfg = load_config()
    if not cfg.get("enabled", True):
        return
    try:
        event = json.load(sys.stdin)
    except Exception:
        return
    if not isinstance(event, dict):
        return
    prompt = event.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        return
    cfg["_prompt"] = prompt.strip()

    items = collect(cfg)
    if not items:
        return

    lines = []
    for e in items:
        slug = e.get("slug", "")
        if cfg.get("with_description", True) and describe(e):
            lines.append(f"- {slug} — {describe(e)}")
        else:
            lines.append(f"- {slug}")

    ctx = (
        "terra memory — possibly relevant entities (pointers, not facts; use the "
        "recall tool for detail, and verify code-sourced claims against current code):\n"
        + "\n".join(lines)
    )
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "UserPromptSubmit",
            "additionalContext": ctx,
        }
    }))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass  # fail-open: never break the user's prompt, whatever happens
