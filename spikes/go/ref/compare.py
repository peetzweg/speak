"""Compare Go --debug-tokens JSON with ref.py JSON (same keys)."""
import json, sys
a, b = (json.loads(open(p, encoding="utf-8").read()) for p in sys.argv[1:3])
ok = True
for k in ("phonemes", "batches", "ids"):
    same = a[k] == b[k]
    ok &= same
    print(f"{k:9} {'MATCH' if same else 'DIFF'}")
    if not same:
        print("  go:", a[k]); print("  py:", b[k])
print("PARITY", "OK" if ok else "FAILED")
sys.exit(0 if ok else 1)
