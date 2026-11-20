"""Export misaki's neural G2P fallback to the ONNX files speak embeds (assets/g2p/en_{us,gb}.onnx).

misaki (hexgrad/misaki, en.py `FallbackNetwork`) reads English words its lexicons don't know with
a tiny BART model: PeterReid/graphemes_to_phonemes_en_us, and _en_gb for British English (both
Apache-2.0). This script downloads both checkpoints at pinned commits, checks their SHA-256s, and
exports one ONNX graph per dialect that speak runs once per decoding step:

    input_ids          int64 [1, graphemes]  [1] + grapheme ids + [2], as misaki builds them
    decoder_input_ids  int64 [1, steps]      [1] + the tokens decoded so far
    logits             float [1, vocab]      scores for the next token

The vocabularies (`grapheme_chars`, `phoneme_chars`) and the provenance go into the ONNX metadata,
so the file is all speak needs. Each export is then checked against PyTorch: greedy decoding
through ONNX Runtime must give transformers' `generate()` output for a set of words, or the script
fails.

Environment (CPU-only, Python 3.14; two runs gave byte-identical files with exactly these):
    python3 -m venv /tmp/g2p-venv
    /tmp/g2p-venv/bin/pip install --index-url https://download.pytorch.org/whl/cpu \
        --extra-index-url https://pypi.org/simple \
        torch==2.15.0 transformers==4.51.2 onnx==1.23.2 onnxruntime==1.32.0
    /tmp/g2p-venv/bin/python -I scripts/export-g2p.py --models /tmp/g2p-models

transformers 4.51.2 is the version the checkpoints were saved with (their config.json says so).
--models is where the checkpoints are downloaded (reused if present and intact); --out defaults to
assets/g2p in this repository. Prints the SHA-256 of every file it writes and whether it is the
committed one.
"""

import argparse
import hashlib
import pathlib
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Hugging Face repo, commit, and the SHA-256 of each file used (the LFS oid for the weights).
CHECKPOINTS = {
    "en_us": (
        "PeterReid/graphemes_to_phonemes_en_us",
        "a5631b285d18d59483c32c0c3379cb9fac924f4b",
        {
            "model.safetensors": "dc4a02e62d4fcb4bb4097ecf00db89b8e1a12a549a52ab6adfbba220b80a55c5",
            "config.json": "8deb3537fb29c63cd9f20d75515ae06e4c92f1b6db0703a2d45bca95b33a53a4",
            "generation_config.json": "79a11240b01c6d34cab0b122b63a2b2dd5938c4d37d1530e14144d27e8ba6272",
        },
    ),
    "en_gb": (
        "PeterReid/graphemes_to_phonemes_en_gb",
        "d8357d5067fa26a5c34134d6bbcf4bbf000c0ac8",
        {
            "model.safetensors": "4994f474bb6f4584076a4e98189caaec11aa3f773a0d82d5bc263a03dd07e703",
            "config.json": "e4f248e6af0cfb6cb54aea6ff0168d16b6f3cdabed438c487cde4ab0815d1bac",
            "generation_config.json": "79a11240b01c6d34cab0b122b63a2b2dd5938c4d37d1530e14144d27e8ba6272",
        },
    ),
}

# The SHA-256 of what this script writes in the environment above: the committed files.
EXPORTED = {
    "en_us": "de3019e9bd976309c38f9ada6e3260b2a4e6c3fe0754e2ec3c1ddda86c1dd71d",
    "en_gb": "1f5d2f38bb305d93e4c1b3b6df96d96fdd9747ca1be091e2d6fc3fc24375e307",
}

OPSET = 17
CHECK_WORDS = ["github", "Kubernetes", "nginx", "localhost", "internationalization", "Kokoro", "-", "", "x" * 62]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def fetch(models, name):
    """The checkpoint directory for `name`, downloaded at its pinned commit and verified."""
    repo, commit, files = CHECKPOINTS[name]
    d = models / name
    d.mkdir(parents=True, exist_ok=True)
    for file, want in files.items():
        path = d / file
        if not path.exists() or sha256(path) != want:
            url = f"https://huggingface.co/{repo}/resolve/{commit}/{file}"
            print(f"downloading {url}", file=sys.stderr)
            urllib.request.urlretrieve(url, path)
        got = sha256(path)
        if got != want:
            sys.exit(f"{path}: SHA-256 {got}, expected {want}")
    return d


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--models", type=pathlib.Path, required=True, help="where to keep the downloaded checkpoints")
    ap.add_argument("--out", type=pathlib.Path, default=ROOT / "assets" / "g2p")
    args = ap.parse_args()

    import numpy as np
    import onnx
    import onnxruntime as ort
    import torch
    import transformers
    from transformers import BartForConditionalGeneration

    args.out.mkdir(parents=True, exist_ok=True)
    for name, (repo, commit, files) in CHECKPOINTS.items():
        checkpoint = fetch(args.models, name)
        # Eager attention exports to plain MatMul/Softmax; misaki loads the default (sdpa), which
        # is what the decoded output is checked against below.
        model = BartForConditionalGeneration.from_pretrained(checkpoint, attn_implementation="eager").eval()
        misaki = BartForConditionalGeneration.from_pretrained(checkpoint).eval()
        cfg = model.config

        class Step(torch.nn.Module):
            """One greedy step without a cache: the whole model, logits of the last position."""

            def __init__(self, m):
                super().__init__()
                self.m = m

            def forward(self, input_ids, decoder_input_ids):
                out = self.m(input_ids=input_ids, decoder_input_ids=decoder_input_ids, use_cache=False, return_dict=True)
                return out.logits[:, -1, :]

        path = args.out / f"{name}.onnx"
        example = (torch.tensor([[1, 20, 21, 2]]), torch.tensor([[1, 1]]))
        with torch.no_grad():
            torch.onnx.export(
                Step(model).eval(),
                example,
                str(path),
                input_names=["input_ids", "decoder_input_ids"],
                output_names=["logits"],
                dynamic_axes={"input_ids": {1: "graphemes"}, "decoder_input_ids": {1: "steps"}},
                opset_version=OPSET,
                do_constant_folding=True,
                dynamo=False,
            )
        graph = onnx.load(str(path))
        meta = {
            "grapheme_chars": cfg.grapheme_chars,
            "phoneme_chars": cfg.phoneme_chars,
            "max_position_embeddings": str(cfg.max_position_embeddings),
            "source": f"https://huggingface.co/{repo}/tree/{commit}",
            "source_sha256": files["model.safetensors"],
            "license": "Apache-2.0",
            "exported_with": f"scripts/export-g2p.py, torch {torch.__version__}, transformers {transformers.__version__}, onnx {onnx.__version__}",
        }
        del graph.metadata_props[:]
        for k, v in meta.items():
            graph.metadata_props.add(key=k, value=v)
        graph.doc_string = f"misaki's neural G2P fallback ({repo}), one greedy decoding step"
        onnx.checker.check_model(graph)
        onnx.save(graph, str(path))
        model.eval()  # the export restores the wrapper's mode, which would leave dropout on

        # The check: greedy decoding through ONNX Runtime, as speak does it, against generate().
        session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
        g2t = {g: i for i, g in enumerate(cfg.grapheme_chars)}
        worst = 0.0
        for word in CHECK_WORDS:
            ids = [1] + [g2t.get(g, 3) for g in word] + [2]
            with torch.no_grad():
                want = misaki.generate(input_ids=torch.tensor([ids]))[0].tolist()
            got = [cfg.decoder_start_token_id]
            while len(got) < 21:  # transformers' default: 20 new tokens, the last one forced to </s>
                logits = session.run(None, {"input_ids": np.array([ids]), "decoder_input_ids": np.array([got])})[0][0]
                with torch.no_grad():
                    ref = model(input_ids=torch.tensor([ids]), decoder_input_ids=torch.tensor([got])).logits[0, -1].numpy()
                worst = max(worst, float(np.abs(logits - ref).max()))
                nxt = 2 if len(got) == 20 else int(np.argmax(logits))
                got.append(nxt)
                if nxt == 2:
                    break
            if got != want:
                sys.exit(f"{name}: ONNX decodes {word!r} to {got}, transformers to {want}")
        digest = sha256(path)
        same = "as committed" if digest == EXPORTED[name] else f"DIFFERS from the committed {EXPORTED[name]}"
        print(f"{path}: {path.stat().st_size} bytes, sha256 {digest} ({same}); max |logit diff| vs PyTorch {worst:.1e}")


if __name__ == "__main__":
    main()
