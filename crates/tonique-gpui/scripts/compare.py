"""Side by side: the UI bench's results for each app, the median of the runs."""
import json, statistics, sys
from pathlib import Path

out = Path(sys.argv[1])
results = {}
for f in sorted(out.glob("*.json")):
    ui = f.stem.rsplit("-", 1)[0]
    results.setdefault(ui, []).append(json.loads(f.read_text()))

uis = [u for u in ("egui", "gpui") if u in results]
first = results[uis[0]][0]
print(f"\n{first['project']} ×{first['scale']}: {first['tracks']} tracks, {first['clips']} clips"
      f" — median of {min(len(results[u]) for u in uis)} run(s)\n")

def med(ui, phase, *path):
    values = []
    for r in results[ui]:
        p = next(x for x in r["phases"] if x["phase"] == phase)
        for k in path:
            p = p[k]
        if p is not None:
            values.append(p)
    return statistics.median(values) if values else float("nan")

rows = [("UI thread CPU / frame p50 (ms)", ("cpu_ms", "p50")),
        ("UI thread CPU / frame p95 (ms)", ("cpu_ms", "p95")),
        ("UI thread CPU / frame p99 (ms)", ("cpu_ms", "p99")),
        ("fps", ("fps",)),
        ("dropped frames", ("dropped",)),
        ("process CPU %", ("process_cpu_percent",))]
phases = [p["phase"] for p in first["phases"]]
w = 9
for label, path in rows:
    print(label)
    print(f"  {'phase':<9}" + "".join(f"{u:>{w}}" for u in uis) + (f"{'gpui/egui':>{w + 2}}" if len(uis) == 2 else ""))
    for ph in phases:
        vals = [med(u, ph, *path) for u in uis]
        ratio = f"{vals[1] / vals[0]:>{w + 1}.2f}×" if len(vals) == 2 and vals[0] else ""
        print(f"  {ph:<9}" + "".join(f"{v:>{w}.2f}" for v in vals) + ratio)
    print()
for u in uis:
    peak = statistics.median([r["peak_rss_mb"] for r in results[u] if r.get("peak_rss_mb")] or [float("nan")])
    print(f"peak memory {u}: {peak:.0f} MB")
