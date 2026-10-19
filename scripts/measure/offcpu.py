"""Off-CPU time per thread from a `perf record -e sched:sched_switch -g` recording.

Each switch-out is paired with the same thread's next switch-in, and the time between
them is attributed to the thread and to the innermost named user-space frame at the
switch-out. Needs root for the tracepoint (sudo perf record ...); reads the file with
perf script, so no root is needed to analyse it.

usage: offcpu.py <perf.data>
"""
import collections, re, subprocess, sys
data = sys.argv[1]
text = subprocess.run(["perf", "script", "-i", data, "--no-inline"], capture_output=True, text=True).stdout
HEAD = re.compile(r"^\S.*?\s+\d+\s+\[\d+\]\s+([\d.]+):\s+sched:sched_switch:\s+(.*?):(\d+)\s+\[\d+\]\s+(\S+)\s+==>\s+(.*?):(\d+)\s+\[\d+\]")
FRAME = re.compile(r"^\s+[0-9a-f]+\s+(.*?)\s+\((.*)\)$")
events = []
cur = None
for line in text.splitlines():
    m = HEAD.match(line)
    if m:
        cur = {"t": float(m.group(1)), "prev": int(m.group(3)), "state": m.group(4), "next": int(m.group(6)), "frames": []}
        events.append(cur)
    elif cur and line.startswith("\t"):
        f = FRAME.match(line)
        if f:
            cur["frames"].append((f.group(1), f.group(2)))
pending = {}
spans = []  # (tid, state, seconds, user frames at switch-out)
for ev in events:
    if ev["prev"] != 0 and ev["prev"] != ev["next"]:
        pending[ev["prev"]] = ev
    prev = pending.pop(ev["next"], None)
    if prev is not None:
        user = [n for n, d in prev["frames"] if "kernel" not in d]
        spans.append((ev["next"], prev["state"], ev["t"] - prev["t"], user))
per_thread = collections.defaultdict(lambda: collections.defaultdict(float))
for tid, state, secs, _ in spans:
    per_thread[tid][state] += secs
cmd = [tid for tid, st in per_thread.items() if any("run_check_project" in " ".join(u) for t, s, d, u in spans if t == tid)]
print(f"threads with off-CPU spans: {len(per_thread)}; command thread: {cmd}")
for tid in sorted(per_thread, key=lambda t: -sum(per_thread[t].values()))[:10]:
    tot = sum(per_thread[tid].values())
    print(f"  tid {tid}: off-CPU {tot:.3f} s  " + ", ".join(f"{k} {v:.3f}" for k, v in sorted(per_thread[tid].items())))
def innermost(user):
    for name in user:
        if "phpstan_rs" in name or "rayon" in name:
            return name[:110]
    return user[0][:110] if user else "(no user frames)"
for label, pick in (("command thread", lambda t: t in cmd), ("pool workers", lambda t: t not in cmd)):
    agg = collections.defaultdict(float)
    for tid, state, secs, user in spans:
        if pick(tid) and state.startswith("S"):
            agg[innermost(user)] += secs
    print(f"\n{label}: sleeping (S) off-CPU by innermost named frame, top 8:")
    for k, v in sorted(agg.items(), key=lambda kv: -kv[1])[:8]:
        print(f"  {v:7.3f} s  {k}")
