"""Reference output of misaki's neural G2P fallback for tests/g2p/words.txt.

Runs misaki's own `FallbackNetwork` class, taken verbatim from misaki/en.py at a pinned commit, on
the pinned checkpoints that scripts/export-g2p.py downloads, and writes reference-us.tsv and
reference-gb.tsv next to this file: word, phonemes, rating (tab-separated; a missing phoneme field
means the model said nothing). speak's port in src/g2p/neural.rs must reproduce them exactly.

Usage, in the venv from scripts/export-g2p.py (torch 2.15.0, transformers 4.51.2), after the
export has downloaded the checkpoints into DIR:
    python -I tests/g2p/reference.py --models DIR
"""

import argparse
import ast
import hashlib
import pathlib
import sys
import types
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "scripts"))  # for export-g2p.py's checkpoint list
sys.dont_write_bytecode = True

# misaki main at the time of writing; en.py last changed in 232a5e3 ("Add a transformer-based
# default english fallback").
MISAKI_COMMIT = "fba1236595f2d2bf21d414ba6e57d25256afada3"
EN_PY_SHA256 = "a9c36c90a3fa6c6084e9042fd8e19f6049add4f1435d8fe5ca1b77f3529a98f4"


def fallback_network_class(models):
    """misaki's FallbackNetwork, compiled from its source, loading the pinned local checkpoints."""
    import torch
    from transformers import BartForConditionalGeneration

    url = f"https://raw.githubusercontent.com/hexgrad/misaki/{MISAKI_COMMIT}/misaki/en.py"
    source = urllib.request.urlopen(url).read()
    got = hashlib.sha256(source).hexdigest()
    if got != EN_PY_SHA256:
        sys.exit(f"{url}: SHA-256 {got}, expected {EN_PY_SHA256}")
    source = source.decode()
    node = next(n for n in ast.parse(source).body if isinstance(n, ast.ClassDef) and n.name == "FallbackNetwork")
    export = __import__("export-g2p")
    local = {repo: str(export.fetch(models, name)) for name, (repo, _, _) in export.CHECKPOINTS.items()}
    pinned = types.SimpleNamespace(from_pretrained=lambda repo: BartForConditionalGeneration.from_pretrained(local[repo]))
    namespace = {"torch": torch, "BartForConditionalGeneration": pinned}
    exec(compile(ast.get_source_segment(source, node), "misaki/en.py", "exec"), namespace)
    return namespace["FallbackNetwork"]


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--models", type=pathlib.Path, required=True, help="checkpoint directory (see scripts/export-g2p.py)")
    args = ap.parse_args()
    FallbackNetwork = fallback_network_class(args.models)
    words = [w for w in (HERE / "words.txt").read_text(encoding="utf-8").splitlines() if w and not w.startswith("#")]
    for dialect, british in [("us", False), ("gb", True)]:
        net = FallbackNetwork(british)
        with open(HERE / f"reference-{dialect}.tsv", "w", encoding="utf-8", newline="\n") as f:
            for word in words:
                phonemes, rating = net(types.SimpleNamespace(text=word))
                f.write(f"{word}\t{phonemes}\t{rating}\n")
        print(f"reference-{dialect}.tsv: {len(words)} words", file=sys.stderr)


if __name__ == "__main__":
    main()
