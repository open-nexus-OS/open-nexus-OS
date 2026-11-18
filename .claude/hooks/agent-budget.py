#!/usr/bin/env python3
"""PreToolUse guard — the token budget: an agent gets a brief, never the conversation.

Why: a strong model at high effort fanning out agents empties the account's token window in
minutes, and a resumed session re-spawns the same agents and empties the next window too
(sprint W45 lost two days to it). The rule is CLAUDE.md <critical id="agent-budget">; this hook
enforces the checkable part:

- Workflow: denied. Multi-agent orchestration is the failure mode itself.
- Agent:
  * `subagent_type: fork` denied — a fork inherits the whole conversation (the most expensive brief).
  * a brief without a repo path, or without a result file (an absolute .md/.json/.txt path the
    agent writes to), denied — the agent would explore, and a resumed session could not tell
    that the work was already done. (web-fetch / claude-code-guide agents are exempt from this.)
  * legwork (sonnet/haiku): `effort` above medium denied; LEGWORK_BURST spawns per BURST_WINDOW,
    LEGWORK_TOTAL per TOTAL_WINDOW.
  * fable runs only through the two definitions in .claude/agents/ — `architect` (decides,
    read-only) and `critical-writer` (one delicate bounded change). Each needs the sections of
    its brief form (a brief is facts, sources and a question — not a hand-over of the chat),
    runs at the definition's effort (high), one strong agent per BURST_WINDOW, and its own cap
    per TOTAL_WINDOW.
  * any other agent on a model that is not sonnet/haiku ASKS the user — never silent.
  * the caps count per user across sessions (the quota is per account, not per session).
    A follow-up to a living agent (SendMessage) is not a spawn and is not counted.
- NEXUS_AGENT_BUDGET=off disables the hook (the user's escape, set in the shell that starts claude).
- NEXUS_AGENT_BUDGET_STATE=<dir> moves the spawn record (the hook's own tests use it).

Fail-open on a hook bug, like protect-zones.py: a broken hook must never block normal work.
"""
import json
import os
import re
import sys
import time

CHEAP_MODELS = ("haiku", "sonnet")
CHEAP_EFFORT = (None, "low", "medium")
BURST_WINDOW = 10 * 60                    # the burst window
TOTAL_WINDOW = 5 * 60 * 60                # the 5-hour quota window
LEGWORK_BURST, LEGWORK_TOTAL = 3, 12      # sonnet/haiku spawns per burst window / per quota window
STRONG_BURST = 1                          # fable agents per burst window, over both kinds

# Fable as an agent: the definition supplies model and effort; the call may repeat them, not raise them.
STRONG_MODELS = (None, "fable")
STRONG_EFFORT = (None, "high")
STRONG_TYPES = {
    "architect": {
        "total": 3,
        "sections": ("Facts", "Sources", "Question"),
    },
    "critical-writer": {
        "total": 2,
        "sections": ("Change", "Invariant", "Files", "Gates"),
    },
}
BRIEF_EXEMPT_TYPES = ("web-fetch", "claude-code-guide", "statusline-setup")

REPO_PATH = re.compile(
    r'(?:^|[\s`\'"(,])(?:source|userspace|tools|docs|tasks|tests|examples|resources|scripts|'
    r'config|recipes|\.claude)/[\w./@-]+|/home/[\w./@-]+'
)
RESULT_FILE = re.compile(r'/(?:home|tmp)/[\w./@-]+\.(?:md|json|txt)\b')


def decide(decision, reason):
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    }))


def state_path():
    d = os.environ.get("NEXUS_AGENT_BUDGET_STATE") or f"/tmp/nexus-agent-budget-{os.getuid()}"
    os.makedirs(d, mode=0o700, exist_ok=True)
    return os.path.join(d, "spawns.json")


def load_spawns(now):
    """The spawn times per kind; the record written before the kinds existed is a plain list of legwork."""
    try:
        with open(state_path(), encoding="utf-8") as f:
            raw = json.load(f)
    except (OSError, ValueError):
        raw = {}
    if isinstance(raw, list):
        raw = {"legwork": raw}
    spawns = {}
    for kind, times in raw.items():
        try:
            spawns[kind] = [float(t) for t in times if now - float(t) < TOTAL_WINDOW]
        except (TypeError, ValueError):
            spawns[kind] = []
    return spawns


def save_spawns(spawns):
    with open(state_path(), "w", encoding="utf-8") as f:
        json.dump(spawns, f)


def wait_minutes(window, times, now):
    return int(window - (now - min(times))) // 60 + 1


def rate_check(kind, now):
    spawns = load_spawns(now)
    mine = spawns.get(kind, [])
    if kind == "legwork":
        recent = [t for t in mine if now - t < BURST_WINDOW]
        if len(recent) >= LEGWORK_BURST:
            return (f"agent budget: {LEGWORK_BURST} legwork agents already started in the last "
                    f"{BURST_WINDOW // 60} min. Do the work yourself now, or wait "
                    f"~{wait_minutes(BURST_WINDOW, recent, now)} min. Check the previous agents' result "
                    f"files before starting another.")
        if len(mine) >= LEGWORK_TOTAL:
            return (f"agent budget: {LEGWORK_TOTAL} legwork agents already started in this "
                    f"{TOTAL_WINDOW // 3600}-hour window. No more until "
                    f"~{wait_minutes(TOTAL_WINDOW, mine, now)} min from now: finish sequentially in the "
                    f"main session.")
    else:
        strong = [t for k in STRONG_TYPES for t in spawns.get(k, [])]
        recent = [t for t in strong if now - t < BURST_WINDOW]
        if len(recent) >= STRONG_BURST:
            return (f"agent budget: a fable agent was started in the last {BURST_WINDOW // 60} min. "
                    f"Continue it with SendMessage (its context is intact) instead of starting another, "
                    f"or wait ~{wait_minutes(BURST_WINDOW, recent, now)} min.")
        total = STRONG_TYPES[kind]["total"]
        if len(mine) >= total:
            return (f"agent budget: {total} `{kind}` agents already started in this "
                    f"{TOTAL_WINDOW // 3600}-hour window (~{wait_minutes(TOTAL_WINDOW, mine, now)} min "
                    f"until the next). Decide in the main session, or tell the user that the package "
                    f"needs a session on fable.")
    spawns.setdefault(kind, []).append(now)
    save_spawns(spawns)
    return None


def missing_sections(prompt, sections):
    return [s for s in sections if not re.search(rf'^\s*##\s*{s}\b', prompt, re.I | re.M)]


def check_brief(sub, prompt):
    if sub in BRIEF_EXEMPT_TYPES:
        return None
    if not REPO_PATH.search(prompt):
        return ("agent budget: the brief names no file or directory. An agent without paths explores "
                "the repo at your expense — point it at the exact files (repo-relative paths).")
    if not RESULT_FILE.search(prompt):
        return ("agent budget: the brief names no result file. Tell the agent the absolute path of the "
                ".md/.json/.txt it writes its result to (scratchpad), so a resumed session finds the "
                "work instead of re-spawning the agent.")
    return None


def check_agent(ti):
    sub = ti.get("subagent_type") or "general-purpose"
    model = ti.get("model")
    effort = ti.get("effort")
    prompt = ti.get("prompt") or ""

    if sub == "fork":
        return "deny", ("agent budget: `fork` inherits the whole conversation — the most expensive brief there is. "
                        "Write a self-contained brief for a general-purpose agent with model: sonnet instead.")

    if sub in STRONG_TYPES and model in STRONG_MODELS:
        if effort not in STRONG_EFFORT:
            return "deny", (f"agent budget: effort `{effort}` is not allowed for `{sub}` — it runs at the "
                            "definition's effort (high). Omit `effort`.")
        reason = check_brief(sub, prompt)
        if reason:
            return "deny", reason
        missing = missing_sections(prompt, STRONG_TYPES[sub]["sections"])
        if missing:
            return "deny", (f"agent budget: the brief for `{sub}` lacks the section(s) "
                            f"{', '.join('## ' + m for m in missing)}. The form is in "
                            f".claude/agents/{sub}.md — fable gets facts, sources and a question, "
                            "not a summary of the chat.")
        reason = rate_check(sub, time.time())
        if reason:
            return "deny", reason
        return None, None

    if effort not in CHEAP_EFFORT:
        return "deny", (f"agent budget: effort `{effort}` is not allowed for a legwork agent (CLAUDE.md "
                        "agent-budget). Agents do specified legwork at low|medium; the thinking stays in "
                        "the main session or goes to the architect.")
    reason = check_brief(sub, prompt)
    if reason:
        return "deny", reason
    if model not in CHEAP_MODELS:
        return "ask", (f"agent budget: this agent would run on `{model or 'the main model (inherited)'}`. "
                       "CLAUDE.md agent-budget: legwork runs on sonnet (searches: haiku), fable only as "
                       "`architect` or `critical-writer`; any other strong-model agent is the user's "
                       "explicit decision. Approve only if the user asked for it in this session; "
                       "otherwise set model: sonnet.")
    reason = rate_check("legwork", time.time())
    if reason:
        return "deny", reason
    return None, None


def main():
    if os.environ.get("NEXUS_AGENT_BUDGET", "").lower() in ("off", "0", "false"):
        return
    payload = json.loads(sys.stdin.read())
    tool = payload.get("tool_name")
    ti = payload.get("tool_input") or {}
    if tool == "Workflow":
        decide("deny", "agent budget: Workflow (multi-agent orchestration) is off in this repo — it is the "
                       "pattern that empties the token window. Do the work sequentially; legwork goes to "
                       "single sonnet agents via Agent. The user can lift this with NEXUS_AGENT_BUDGET=off.")
        return
    if tool != "Agent":
        return
    decision, reason = check_agent(ti)
    if decision:
        decide(decision, reason)


if __name__ == "__main__":
    try:
        main()
    except Exception as e:  # fail-open: never block normal work on a hook bug
        sys.stderr.write(f"agent-budget hook error (failing open): {e}\n")
    sys.exit(0)
