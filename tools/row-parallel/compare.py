"""Compare one fixed PQ2/LoRA forward step with one and four Rust row workers.

Evaluation timings exclude model loading and capture I/O. They are one-sample
observations, not application TPS, statistical benefit or promotion evidence.
"""
import argparse
from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
import struct

LOCK = Path(__file__).resolve().parents[2] / "config/swift-assessment.lock.json"
MODEL = json.loads(LOCK.read_text(encoding="utf-8"))["model"]["sha256"]
ADAPTER = "f1669534803d340a496015f5c45125f3437b4d13ec764f40e34488ce83967f42"
GRAPH_IDENTITY = (MODEL + ":qwen35-text-v1:nonflash-f16:kv-f16:gdn-f32:prism-arm64-fused-v1"
                  + ":rank-one-lora-v1:" + ADAPTER + ":3fa66666")
CANDIDATE_WORKERS = 4


@dataclass(frozen=True)
class Geometry:
    layers: int = 64
    width: int = 1024
    convolution: int = 30720
    recurrent: int = 786432
    vocabulary: int = 248320


GEOMETRY = Geometry()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def bounded_bytes(path, limit):
    path = Path(path)
    metadata = path.lstat()
    require(not path.is_symlink() and not getattr(metadata, "st_file_attributes", 0) & 0x400,
            "reparse capture input")
    require(path.is_file() and 0 < metadata.st_size <= limit, "capture input exceeds byte bound")
    with path.open("rb") as stream:
        payload = stream.read(limit + 1)
    require(0 < len(payload) <= limit, "capture input grew beyond byte bound")
    return payload


def ordinary_directory(path):
    metadata = path.lstat()
    require(path.is_dir() and not path.is_symlink()
            and not getattr(metadata, "st_file_attributes", 0) & 0x400,
            "capture directory or ancestor is a reparse point")


def payload_inventory(directory):
    require(directory.is_absolute() and len(directory.parents) <= 64,
            "capture directory must be a bounded absolute path")
    for ancestor in (directory, *directory.parents):
        ordinary_directory(ancestor)
    state = directory / "state-0"
    ordinary_directory(state)
    payloads = set()
    for parent, limit in ((directory, 32), (state, 128)):
        for index, path in enumerate(parent.iterdir()):
            require(index < limit, "capture directory entry bound exceeded")
            metadata = path.lstat()
            require(not path.is_symlink() and not getattr(metadata, "st_file_attributes", 0) & 0x400,
                    "reparse capture entry")
            if path.is_dir():
                require(parent == directory and path == state, "unexpected nested capture directory")
            else:
                require(path.is_file(), "nonregular capture entry")
                if parent == state:
                    require(path.suffix == ".f32le", "unexpected state component")
                if path.suffix == ".f32le":
                    payloads.add(path.relative_to(directory).as_posix())
    return payloads


def controls(directory, workers, geometry):
    records = [json.loads(line) for line in bounded_bytes(directory / "stdout.txt", 65536).splitlines()
               if line]
    weights = [item for item in records if item.get("event") == "weights-loaded"]
    adapters = [item for item in records if item.get("event") == "rank-one-adapter-loaded"]
    steps = [item for item in records if item.get("event") == "forward-step"]
    require(not any(item.get("event") == "error" for item in records), "checker reported an error")
    require(len(weights) == 1 and weights[0]["model_sha256"] == MODEL
            and weights[0]["weight_representation"] == "PQ2_0", "PQ2 model control changed")
    require(len(adapters) == 1 and adapters[0]["adapter_sha256"] == ADAPTER
            and adapters[0]["model_sha256"] == MODEL and adapters[0]["target_pairs"] == 129
            and adapters[0]["scale_bits"] == struct.pack(">f", 1.3).hex(), "adapter control changed")
    require(len(steps) == 1, "expected exactly one fixed forward step")
    step = steps[0]
    require(all(type(step[key]) is int for key in ("position", "input_token", "logit_count"))
            and step["position"] == 0 and step["input_token"] == 248045
            and step["backend_requested"] == "auto" and step["logit_count"] == geometry.vocabulary,
            "forward controls changed")
    require(step["identity"] == GRAPH_IDENTITY, "graph identity changed")
    require(type(step["logits"]) is str and Path(step["logits"]).is_absolute()
            and Path(step["logits"]) == directory / "logits-0.f32le", "emitted logits path changed")
    require(type(step["checkpoint_bytes"]) is int and 0 < step["checkpoint_bytes"] <= 12348030976,
            "invalid checkpoint extent")
    require(type(step["greedy_token"]) is int and 0 <= step["greedy_token"] < geometry.vocabulary,
            "invalid greedy token")
    require(type(step["row_workers"]) is int and step["row_workers"] == workers,
            "row worker control changed")
    operations = step["parallel_row_operations"]
    require(type(operations) is int and operations >= 0, "invalid parallel operation count")
    require((workers == 1 and operations == 0) or (workers == CANDIDATE_WORKERS and operations > 0),
            "requested row dispatch was not observed")
    milliseconds = step["evaluation_milliseconds"]
    require(type(milliseconds) in (int, float) and math.isfinite(milliseconds) and 0 < milliseconds <= 240000,
            "invalid fixed-step evaluation duration")
    return step


def inventory(geometry):
    expected = {"logits-0.f32le": geometry.vocabulary}
    for block in range(geometry.layers):
        components = (("key-0", geometry.width), ("value-0", geometry.width)) if block % 4 == 3 else (
            ("convolution", geometry.convolution), ("recurrent", geometry.recurrent))
        for label, elements in components:
            expected[f"state-0/block-{block}-{label}.f32le"] = elements
    return expected


def compare(serial, parallel, geometry=GEOMETRY):
    serial, parallel = Path(serial), Path(parallel)
    expected = inventory(geometry)
    for directory in (serial, parallel):
        require(payload_inventory(directory) == set(expected), "complete logical state inventory changed")
    first = controls(serial, 1, geometry)
    second = controls(parallel, CANDIDATE_WORKERS, geometry)
    require(first["identity"] == second["identity"], "graph identity changed")
    require(first["greedy_token"] == second["greedy_token"], "greedy token changed")
    require(first["checkpoint_bytes"] == second["checkpoint_bytes"], "checkpoint extent changed")
    files = []
    for relative, elements in expected.items():
        first_payload = bounded_bytes(serial / relative, elements * 4)
        second_payload = bounded_bytes(parallel / relative, elements * 4)
        require(len(first_payload) == elements * 4 and len(second_payload) == elements * 4,
                "component extent changed")
        require(first_payload == second_payload, "serial/parallel arithmetic bits differ: " + relative)
        require(all(math.isfinite(value) for (value,) in struct.iter_unpack("<f", first_payload)),
                "nonfinite compared value")
        if relative == "logits-0.f32le":
            greedy = max(enumerate(struct.iter_unpack("<f", first_payload)), key=lambda item: item[1][0])[0]
            require(first["greedy_token"] == greedy, "greedy token does not match compared logits")
        files.append({"path": relative, "elements": elements,
                      "sha256": hashlib.sha256(first_payload).hexdigest()})
    return {
        "status": "fixed-step-serial-parallel-byte-exact",
        "modelSha256": MODEL, "adapterSha256": ADAPTER, "adapterScale": 1.3,
        "suppliedTokens": [248045], "rowWorkers": [1, CANDIDATE_WORKERS], "backend": "auto",
        "files": len(files), "elements": sum(item["elements"] for item in files),
        "serialEvaluationMilliseconds": first["evaluation_milliseconds"],
        "parallelEvaluationMilliseconds": second["evaluation_milliseconds"],
        "observedEvaluationSpeedup": first["evaluation_milliseconds"] / second["evaluation_milliseconds"],
        "parallelRowOperations": second["parallel_row_operations"],
        "timingScope": "One fixed forward evaluation per condition; excludes loading and capture I/O. No application TPS, statistical benefit, prefill or performance qualification.",
        "performanceGatePassed": False, "defaultMayChange": False, "comparisons": files,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--serial", required=True, type=Path)
    parser.add_argument("--parallel", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    report = compare(arguments.serial, arguments.parallel)
    with arguments.output.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2, allow_nan=False)
        stream.write("\n")
    print(json.dumps({key: value for key, value in report.items() if key != "comparisons"}, allow_nan=False))


if __name__ == "__main__":
    main()
