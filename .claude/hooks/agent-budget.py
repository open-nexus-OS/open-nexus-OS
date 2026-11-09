#!/usr/bin/env python3
"""PreToolUse guard — the token budget: agents are legwork on a cheap model, one at a time.

Why: a strong model at high effort fanning out agents empties the account's token window in
minutes, and a resumed session re-spawns the same agents and empties the next window too
(sprint W45 lost two days to it). The rule is CLAUDE.md <critical id="agent-budget">; this hook
enforces the checkable part:

- Workflow: denied. Multi-agent orchestration is the failure mode itself.
- Agent:
  * `subagent_type: fork` denied — a fork inherits the whole conversation (the most expensive brief).
  * `effort` above medium denied.
  * a brief without a repo path, or without a result file (an absolute .md/.json/.txt path the
    agent writes to), denied — the agent would explore, and a resumed session could not tell
    that the work was already done. (web-fetch / claude-code-guide agents are exempt from this.)
  * a model other than sonnet/haiku ASKS the user — a strong-model agent is the user's call, never silent.
  * a rate cap per user across sessions (the quota is per account, not per session):
    BURST spawns per BURST_WINDOW, TOTAL per TOTAL_WINDOW; over the cap → denied with the wait.
- NEXUS_AGENT_BUDGET=off disables the hook (the user's escape, set in the shell that starts claude).

Fail-open on a hook bug, like protect-zones.py: a broken hook must never block normal work.
"""
import json
import os
import re
import sys
import time

CHEAP_MODELS = ("haiku", "sonnet")
CHEAP_EFFORT = (None, "low", "medium")
BURST, BURST_WINDOW = 2, 10 * 60          # spawns per 10 minutes
TOTAL, TOTAL_WINDOW = 8, 5 * 60 * 60      # spawns per 5-hour quota window
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
    d = f"/tmp/nexus-agent-budget-{os.getuid()}"
    os.makedirs(d, mode=0o700, exist_ok=True)
    return os.path.join(d, "spawns.json")


def load_spawns(now):
    try:
        with open(state_path(), encoding="utf-8") as f:
            spawns = [float(t) for t in json.load(f)]
    except (OSError, ValueError):
        spawns = []
    return [t for t in spawns if now - t < TOTAL_WINDOW]


def save_spawns(spawns):
    with open(state_path(), "w", encoding="utf-8") as f:
        json.dump(spawns, f)


def rate_check(now):
    spawns = load_spawns(now)
    recent = [t for t in spawns if now - t < BURST_WINDOW]
    if len(recent) >= BURST:
        wait = int(BURST_WINDOW - (now - min(recent))) // 60 + 1
        return (f"agent budget: {BURST} agents already started in the last {BURST_WINDOW // 60} min "
                f"(CLAUDE.md agent-budget: one at a time). Do the work yourself now, or wait ~{wait} min. "
                f"Check the previous agents' result files before starting another.")
    if len(spawns) >= TOTAL:
        wait = int(TOTAL_WINDOW - (now - min(spawns))) // 60 + 1
        return (f"agent budget: {TOTAL} agents already started in this {TOTAL_WINDOW // 3600}-hour window. "
                f"No more agents until ~{wait} min from now: finish sequentially in the main session.")
    spawns.append(now)
    save_spawns(spawns)
    return None


def check_agent(ti):
    sub = ti.get("subagent_type") or "general-purpose"
    model = ti.get("model")
    effort = ti.get("effort")
    prompt = ti.get("prompt") or ""

    if sub == "fork":
        return "deny", ("agent budget: `fork` inherits the whole conversation — the most expensive brief there is. "
                        "Write a self-contained brief for a general-purpose agent with model: sonnet instead.")
    if effort not in CHEAP_EFFORT:
        return "deny", (f"agent budget: effort `{effort}` is not allowed for an agent (CLAUDE.md agent-budget). "
                        "Agents do specified legwork at low|medium; the thinking stays in the main session.")
    if sub not in BRIEF_EXEMPT_TYPES:
        if not REPO_PATH.search(prompt):
            return "deny", ("agent budget: the brief names no file or directory. An agent without paths explores "
                            "the repo at your expense — point it at the exact files (repo-relative paths).")
        if not RESULT_FILE.search(prompt):
            return "deny", ("agent budget: the brief names no result file. Tell the agent the absolute path of the "
                            ".md/.json/.txt it writes its result to (scratchpad), so a resumed session finds the "
                            "work instead of re-spawning the agent.")
    if model not in CHEAP_MODELS:
        return "ask", (f"agent budget: this agent would run on `{model or 'the main model (inherited)'}`. "
                       "CLAUDE.md agent-budget: agents run on sonnet (searches: haiku); a strong-model agent is "
                       "the user's explicit decision. Approve only if the user asked for a strong-model agent "
                       "in this session; otherwise set model: sonnet.")
    reason = rate_check(time.time())
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
                       "pattern that empties the token window. Do the work sequentially; at most one sonnet "
                       "agent at a time via Agent. The user can lift this with NEXUS_AGENT_BUDGET=off.")
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
