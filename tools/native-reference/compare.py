"""Strict same-format one-token smoke; no inference or benchmark execution."""
import argparse
from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
import struct

MAX_STATE = 512 * 1024 * 1024
MODEL = "e658f0f0736a03a4549535a439f9b07254ebf8ab88ae27cbe6e52d098ca5d1f5"
NATIVE = "73bfe6cc485554822f80c473a3c0c4f25e80f41b"
ADAPTER = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"


@dataclass(frozen=True)
class Layout:
    attention: tuple = tuple(range(3, 64, 4))
    recurrent: tuple = tuple(i for i in range(64) if i % 4 != 3)
    layers: int = 64
    width: int = 1024
    convolution: int = 30720
    matrix: int = 786432
    vocabulary: int = 248320


LAYOUT = Layout()
TOKENS = (248045,)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def read_bounded(path, limit):
    require(0 < Path(path).stat().st_size <= limit, "capture input exceeds pre-read byte bound")
    with Path(path).open("rb") as file:
        data = file.read(limit + 1)
    require(len(data) <= limit, "capture input grew beyond byte bound")
    return data


class Reader:
    def __init__(self, file):
        self.file = file

    def take(self, size):
        require(0 <= size <= MAX_STATE, "invalid native field byte count")
        data = self.file.read(size)
        require(len(data) == size, "truncated native sequence state")
        return data

    def value(self, code):
        return struct.unpack("<" + code, self.take(struct.calcsize("<" + code)))[0]


def native_components(path, position, layout=LAYOUT):
    """Decode only the exact non-replay single-sequence 73bfe6cc schema.

    Source: llama_context::state_seq_write_data -> llama_memory_hybrid::state_write,
    llama_kv_cache::state_write_{meta,data}, llama_memory_recurrent::state_write_data.
    FP16 V may be serialized by channel; return each logical token row in order.
    """
    require(position in (0, 1), "decoder position is outside its bounded smoke/fixture extent")
    require(0 < Path(path).stat().st_size <= MAX_STATE, "native state exceeds byte bound")
    count = position + 1
    with Path(path).open("rb") as file:
        reader = Reader(file)
        require(reader.value("I") == 0xAF143CD8, "native sequence-state magic changed")
        require(reader.value("i") == 0, "native sequence-state identity changed")
        require(reader.value("I") == 1, "native attention stream count changed")
        require(reader.value("I") == count, "native live attention cell count changed")
        for token in range(count):
            require(reader.value("i") == token, "native attention position changed")
            require(reader.value("I") == 1, "native attention sequence count changed")
            require(reader.value("i") == token and reader.value("i") == token,
                    "native text IMRoPE spatial positions changed")
            require(reader.value("i") == 0, "native attention sequence identity changed")
        transposed = reader.value("I")
        require(transposed in (0, 1), "unknown native V storage order")
        require(reader.value("I") == len(layout.attention), "native attention layer count changed")
        for block in layout.attention:
            require(reader.value("i") == 1, "native K is not binary16")
            require(reader.value("Q") == layout.width * 2, "native K row size changed")
            for token in range(count):
                yield f"block-{block}-key-{token}.f32le", reader.take(layout.width * 2), "e"
        for block in layout.attention:
            require(reader.value("i") == 1, "native V is not binary16")
            if transposed:
                require(reader.value("I") == 2 and reader.value("I") == layout.width,
                        "native transposed V geometry changed")
                raw = reader.take(layout.width * count * 2)
                for token in range(count):
                    row = b"".join(raw[(channel * count + token) * 2:(channel * count + token + 1) * 2]
                                   for channel in range(layout.width))
                    yield f"block-{block}-value-{token}.f32le", row, "e"
            else:
                require(reader.value("Q") == layout.width * 2, "native V row size changed")
                for token in range(count):
                    yield f"block-{block}-value-{token}.f32le", reader.take(layout.width * 2), "e"
        require(reader.value("I") == 1, "native recurrent cell count changed")
        require(reader.value("i") == position and reader.value("I") == 0,
                "native recurrent position/sequence metadata changed")
        require(reader.value("I") == 0, "replay/transposed recurrent state is unsupported")
        require(reader.value("I") == layout.layers, "native recurrent architecture changed")
        for component, elements in (("convolution", layout.convolution), ("recurrent", layout.matrix)):
            for block in layout.recurrent:
                require(reader.value("i") == 0, "native recurrent payload is not binary32")
                require(reader.value("Q") == elements * 4, "native recurrent row size changed")
                yield f"block-{block}-{component}.f32le", reader.take(elements * 4), "f"
        require(file.read(1) == b"", "unconsumed trailing native state bytes")


def compare_payload(reference, code, actual_path):
    """Keep fixed tolerance and finite-value checks independent of token equality."""
    width = struct.calcsize("<" + code)
    require(len(reference) % width == 0, "reference component alignment changed")
    elements = len(reference) // width
    require(Path(actual_path).stat().st_size == elements * 4, "Rust component extent changed")
    failed = 0
    maximum = 0.0
    exact = True
    with Path(actual_path).open("rb") as actual:
        for start in range(0, elements, 4096):
            size = min(4096, elements - start)
            expected = reference[start * width:(start + size) * width]
            observed = actual.read(size * 4)
            require(len(observed) == size * 4, "truncated Rust component")
            canonical = expected if code == "f" else b"".join(
                struct.pack("<f", value[0]) for value in struct.iter_unpack("<e", expected))
            exact &= canonical == observed
            for (ref,), (value,) in zip(struct.iter_unpack("<" + code, expected),
                                       struct.iter_unpack("<f", observed), strict=True):
                require(math.isfinite(ref) and math.isfinite(value), "nonfinite compared value")
                difference = abs(value - ref)
                maximum = max(maximum, difference)
                failed += difference > 1e-3 + 1e-3 * abs(ref)
        require(actual.read(1) == b"", "trailing Rust component bytes")
    return {"elements": elements, "failedElements": failed, "maximumAbsoluteError": maximum,
            "byteExact": exact}


def validate_rust_controls(rust, capture, backend):
    records = [json.loads(line) for line in read_bounded(rust / "stdout.txt", 65536).splitlines() if line]
    weights = [r for r in records if r.get("event") == "weights-loaded"]
    steps = [r for r in records if r.get("event") == "forward-step"]
    adapters = [r for r in records if r.get("event") == "rank-one-adapter-loaded"]
    require(len(weights) == 1 and weights[0]["model_sha256"] == MODEL
            and weights[0]["weight_representation"] == "current-repacked-PTQ1_0",
            "Rust capture used a different artifact")
    require([s["position"] for s in steps] == [0]
            and [s["input_token"] for s in steps] == list(TOKENS)
            and all(s["backend_requested"] == backend for s in steps),
            "Rust supplied tokens/backend differ from comparison controls")
    require(type(capture["adapterLoaded"]) is bool, "invalid native adapter control")
    if capture["adapterLoaded"]:
        bits = struct.pack(">f", capture["adapterScale"]).hex()
        require(len(adapters) == 1 and adapters[0]["adapter_sha256"] == ADAPTER
                and adapters[0]["target_pairs"] == 129 and adapters[0]["scale_bits"] == bits
                and adapters[0]["model_sha256"] == MODEL, "Rust/native adapter identity/scale differs")
    else:
        require(not adapters and capture["adapterScale"] == 0, "native/Rust baseline adapter differs")
    return steps


def compare(native, rust, backend):
    native, rust = Path(native), Path(rust)
    native_inventory = {file.relative_to(native).as_posix() for file in native.rglob("*")
                        if file.is_file() and file.suffix in (".f32le", ".native-seq")}
    require(native_inventory == {"logits-0.f32le", "state-0.native-seq"}, "native payload inventory changed")
    capture = json.loads(read_bounded(native / "native-capture.json", 65536))
    require(capture["nativeCommit"] == NATIVE and capture["modelSha256"] == MODEL,
            "native capture identity differs from exact same-format control")
    require(capture["contextTokens"] == 8192 and capture["threads"] == 4,
            "native capture controls changed")
    require([step["inputToken"] for step in capture["steps"]] == list(TOKENS),
            "native supplied trajectory changed")
    require([step["position"] for step in capture["steps"]] == [0]
            and capture["steps"][0]["logitsCount"] == LAYOUT.vocabulary,
            "native smoke position/logits geometry changed")
    require(capture["adapterLoaded"] is True and capture["adapterScale"] == 1.3, "native smoke adapter control changed")
    steps = validate_rust_controls(rust, capture, backend)
    files = []
    expected = set()
    for position in range(1):
        logits = f"logits-{position}.f32le"
        reference = read_bounded(native / logits, LAYOUT.vocabulary * 4)
        require(len(reference) == LAYOUT.vocabulary * 4, "native logits extent changed")
        files.append({"path": logits, "nativeSha256": digest(native / logits),
                      "rustSha256": digest(rust / logits), **compare_payload(reference, "f", rust / logits)})
        expected.add(logits)
        state = native / f"state-{position}.native-seq"
        require(state.stat().st_size == capture["steps"][position]["sequenceStateBytes"],
                "native serialized state receipt extent changed")
        for name, reference, code in native_components(state, position):
            relative = f"state-{position}/{name}"
            expected.add(relative)
            files.append({"path": relative, "rustSha256": digest(rust / relative),
                          **compare_payload(reference, code, rust / relative)})
    observed = {file.relative_to(rust).as_posix() for file in rust.rglob("*.f32le")}
    require(observed == expected, "Rust complete-state inventory differs from exact native extent")
    failed = sum(file["failedElements"] for file in files)
    greedy = [s["greedy_token"] for s in steps] == [s["greedyToken"] for s in capture["steps"]]
    return {"schemaVersion": 1, "status": "failed" if failed or not greedy else "passed-bounded-one-token-smoke",
            "nativeCommit": NATIVE, "modelSha256": MODEL, "absoluteTolerance": 1e-3,
            "relativeTolerance": 1e-3, "files": len(files), "elements": sum(f["elements"] for f in files),
            "failedElements": failed, "byteExactFiles": sum(f["byteExact"] for f in files),
            "nativeCaptureSha256": digest(native / "native-capture.json"),
            "nativeStateSha256": [digest(native / "state-0.native-seq")],
            "maximumAbsoluteError": max(f["maximumAbsoluteError"] for f in files), "comparisons": files,
            "backendRequested": backend, "greedyTokensMatch": greedy,
            "rustStdoutSha256": digest(rust / "stdout.txt"),
            "writerBasisQualified": False, "qualityQualified": False, "defaultMayChange": False,
            "pendingControls": ["no-adapter", "loaded-zero-identity", "nonzero-change", "scalar", "explicit-neon",
                                "writer-authoring-basis", "quality", "performance", "full-context"]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", required=True, type=Path)
    parser.add_argument("--rust", required=True, type=Path)
    parser.add_argument("--backend", required=True, choices=("auto",))
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    report = compare(arguments.native, arguments.rust, arguments.backend)
    with arguments.output.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2, allow_nan=False)
        output.write("\n")
    raise SystemExit(1 if report["status"] == "failed" else 0)
