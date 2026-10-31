"""Bounded, offline PQ2/Q2_0 weight-layout audit; never executes inference."""

import argparse
import hashlib
import json
from pathlib import Path
import struct


PQ2_WEIGHTS = 128
PQ2_BYTES = 34
Q2_WEIGHTS = 64
Q2_BYTES = 18
MAX_JSON_BYTES = 1024 * 1024
MAX_JSON_DEPTH = 64
MAX_PAYLOAD_BYTES = 1024 * 1024
MAX_U64 = (1 << 64) - 1
NATIVE_COMMIT = "73bfe6cc485554822f80c473a3c0c4f25e80f41b"
SOURCE_BASE = "https://github.com/PrismML-Eng/llama.cpp/blob/" + NATIVE_COMMIT
LIMITATIONS = [
    "Weight layout only; no NPU dispatch, signed deployment or speed qualification.",
    "Q2_0 Q8_0 activation arithmetic does not establish PQ2 Q8_K/odd-group parity.",
    "No GGUF rewrite; transform/sign arrays, tensor names and grouped basis stay unchanged.",
    "Model payload, logits, complete state, adapters and lifecycle remain unqualified.",
    "Storage totals exclude alignment changes, simultaneous source copies and device scratch.",
]


def require(condition, message):
    """Raise a stable input-validation error without silently repairing input."""
    if not condition:
        raise ValueError(message)


def bounded_read(path, limit):
    """Read at most limit+1 bytes; a size check alone cannot bound a growing file."""
    with Path(path).open("rb") as stream:
        data = stream.read(limit + 1)
    require(len(data) <= limit, "input exceeds bounded audit size")
    return data


def finite_scale(raw):
    """Reject both infinities and every NaN, including zero-weight blocks."""
    require(len(raw) == 2, "scale requires exactly two little-endian bytes")
    bits = int.from_bytes(raw, "little")
    require(bits & 0x7C00 != 0x7C00, "nonfinite FP16 scale")
    return struct.unpack("<e", raw)[0]


def validate_extent(data, columns, rows):
    """Require complete native rows; a partial last block is never padded."""
    require(type(columns) is int and type(rows) is int, "integer dimensions required")
    require(columns > 0 and columns % PQ2_WEIGHTS == 0 and rows > 0,
            "positive rows and columns divisible by 128 required")
    require(len(data) <= MAX_PAYLOAD_BYTES, "payload exceeds bounded audit size")
    require(len(data) == columns // PQ2_WEIGHTS * rows * PQ2_BYTES,
            "payload extent does not match complete PQ2 rows")


def bridge_pq2(data, columns, rows=1):
    """Duplicate raw scales and split packed symbols without requantization."""
    validate_extent(data, columns, rows)
    output = bytearray()
    for offset in range(0, len(data), PQ2_BYTES):
        block = data[offset:offset + PQ2_BYTES]
        finite_scale(block[:2])
        output.extend(block[:18])
        output.extend(block[:2])
        output.extend(block[18:])
    return bytes(output)


def restore_pq2(data, columns, rows=1):
    """Reverse only a paired Q2_0 stream whose scale bits are identical."""
    require(type(columns) is int and type(rows) is int and columns > 0
            and columns % PQ2_WEIGHTS == 0 and rows > 0, "invalid paired Q2_0 dimensions")
    require(len(data) <= MAX_PAYLOAD_BYTES // PQ2_BYTES * Q2_BYTES * 2,
            "paired payload exceeds bounded audit size")
    require(len(data) == columns // PQ2_WEIGHTS * rows * Q2_BYTES * 2,
            "payload extent does not match paired Q2_0 rows")
    output = bytearray()
    for offset in range(0, len(data), Q2_BYTES * 2):
        pair = data[offset:offset + Q2_BYTES * 2]
        finite_scale(pair[:2])
        require(pair[:2] == pair[18:20], "paired raw scale bits differ")
        output.extend(pair[:18])
        output.extend(pair[20:])
    return bytes(output)


def decode_pq2_bits(data):
    """Reference element traversal returns exact f32 bits, including signed zero."""
    require(len(data) % PQ2_BYTES == 0, "incomplete PQ2 block")
    result = bytearray()
    for offset in range(0, len(data), PQ2_BYTES):
        block = data[offset:offset + PQ2_BYTES]
        scale = finite_scale(block[:2])
        for index in range(PQ2_WEIGHTS):
            code = (block[2 + index // 4] >> (2 * (index % 4))) & 3
            result.extend(struct.pack("<f", (code - 1) * scale))
    return bytes(result)


def decode_q2_bits(data):
    """Independent byte traversal of the pinned Q2_0 dequantization contract."""
    require(len(data) % Q2_BYTES == 0, "incomplete Q2_0 block")
    result = bytearray()
    for offset in range(0, len(data), Q2_BYTES):
        scale = finite_scale(data[offset:offset + 2])
        for packed in data[offset + 2:offset + Q2_BYTES]:
            for shift in (0, 2, 4, 6):
                result.extend(struct.pack("<f", (((packed >> shift) & 3) - 1) * scale))
    return bytes(result)


def audit_payload(data, columns, rows=1):
    """Check reversible bytes and bitwise decoded weights for a bounded fixture."""
    bridged = bridge_pq2(data, columns, rows)
    require(restore_pq2(bridged, columns, rows) == data, "raw layout roundtrip failed")
    require(decode_pq2_bits(data) == decode_q2_bits(bridged), "decoded f32 bits differ")
    codes = [0, 0, 0, 0]
    for offset in range(0, len(data), PQ2_BYTES):
        for packed in data[offset + 2:offset + PQ2_BYTES]:
            for shift in (0, 2, 4, 6):
                codes[(packed >> shift) & 3] += 1
    return {"Columns": columns, "Rows": rows, "SourceBytes": len(data),
            "BridgedBytes": len(bridged), "SourceSha256": hashlib.sha256(data).hexdigest(),
            "BridgedSha256": hashlib.sha256(bridged).hexdigest(), "RawCodeCounts": codes,
            "RawRoundtrip": "exact", "DecodedF32Bits": "exact"}


def integer_field(record, name):
    """Reject non-u64 counts rather than coercing JSON values."""
    value = record.get(name)
    require(type(value) is int and 0 <= value <= MAX_U64, "invalid integer field: " + name)
    return value


def storage_plan(metadata, census):
    """Reconcile retained inventory; this does not inspect the actual model file."""
    require(type(metadata) is dict and type(census) is dict, "inventory objects required")
    require(type(metadata.get("SchemaVersion")) is int and metadata["SchemaVersion"] == 1
            and type(census.get("SchemaVersion")) is int and census["SchemaVersion"] == 1,
            "unsupported inventory schema")
    require(metadata.get("Format") == "GGUF" and metadata.get("ByteOrder") == "little",
            "expected little-endian GGUF metadata")
    artifact = metadata.get("ArtifactSha256", "")
    require(type(artifact) is str and len(artifact) == 64
            and all(char in "0123456789abcdef" for char in artifact), "invalid artifact digest")
    for record in (metadata, census):
        for name in ("ModelRepository", "ModelRevision"):
            identity = record.get(name)
            require(type(identity) is str and 0 < len(identity) <= 512
                    and all(ord(char) >= 32 and ord(char) != 127 for char in identity),
                    "invalid string identity: " + name)
    for name in ("ArtifactSha256", "ModelRepository", "ModelRevision"):
        require(metadata.get(name) == census.get(name) and metadata.get(name),
                "inventory identity mismatch: " + name)
    require(census.get("CodecCommit") == NATIVE_COMMIT
            and metadata.get("Dialect") == "Prism-" + NATIVE_COMMIT,
            "inventory codec does not match the pinned native source")
    file_bytes = integer_field(metadata, "FileBytes")
    require(file_bytes == integer_field(census, "ArtifactBytes"), "artifact byte mismatch")
    blocks = integer_field(census, "Blocks")
    require(blocks > 0, "empty PQ2 inventory")
    elements = integer_field(census, "Elements")
    source_bytes = integer_field(census, "SerializedBytesRead")
    require(elements == blocks * PQ2_WEIGHTS and source_bytes == blocks * PQ2_BYTES,
            "PQ2 inventory geometry mismatch")
    code_counts = [integer_field(census, name) for name in
                   ("CodeMinusOne", "CodeZero", "CodePlusOne", "CodePlusTwo")]
    require(sum(code_counts) == elements, "raw code census mismatch")
    require(integer_field(census, "NonfiniteScaleBlocks") == 0, "nonfinite scale inventory")
    require(integer_field(census, "ZeroScaleBlocks") <= blocks, "invalid zero-scale count")
    histogram = metadata.get("TensorTypeHistogram", [])
    require(type(histogram) is list and all(type(entry) is dict for entry in histogram),
            "missing or malformed tensor histogram")
    require(all(type(entry.get("TypeId")) is int and 0 <= entry["TypeId"] <= MAX_U64
                for entry in histogram), "invalid tensor type identity")
    matches = [entry for entry in histogram if entry["TypeId"] == 142]
    require(len(matches) == 1, "expected one PQ2 tensor histogram entry")
    pq2 = matches[0]
    tensor_count = integer_field(census, "TensorCount")
    require(0 < tensor_count <= blocks, "positive PQ2 tensor count required")
    require(pq2.get("Name") == "PQ2_0" and integer_field(pq2, "Bytes") == source_bytes
            and integer_field(pq2, "Elements") == elements
            and integer_field(pq2, "Count") == tensor_count,
            "tensor histogram and census disagree")
    scalars = metadata.get("Scalars", {})
    require(type(scalars) is dict, "missing transform metadata")
    required_transform = {"prism.hadamard.version": 1, "prism.hadamard.block_size": 1024,
                          "prism.hadamard.transform": "normalized-sylvester-walsh-hadamard",
                          "prism.hadamard.axis": "input-last-dimension",
                          "prism.hadamard.sign_mode": "explicit", "prism.hadamard.gdn_v_grouped": True}
    require(all(type(scalars.get(key)) is type(value) and scalars[key] == value
                for key, value in required_transform.items()),
            "unsupported or missing Swift transform identity")
    arrays = metadata.get("Arrays", {})
    require(type(arrays) is dict, "missing transform arrays")
    require(type(arrays.get("prism.hadamard.sign_values")) is dict
            and integer_field(arrays["prism.hadamard.sign_values"], "Count") > 0,
            "missing explicit sign inventory")
    bridged_bytes = blocks * Q2_BYTES * 2
    require(file_bytes >= source_bytes, "payload exceeds artifact")
    estimated_file_bytes = file_bytes + bridged_bytes - source_bytes
    require(blocks * 2 <= MAX_U64 and bridged_bytes <= MAX_U64
            and estimated_file_bytes <= MAX_U64, "bridged storage exceeds u64 extent")
    return {"ArtifactSha256": artifact, "InventoryOnly": True, "Pq2Blocks": blocks,
            "Q2Blocks": blocks * 2, "Elements": elements, "SourcePackedBytes": source_bytes,
            "BridgedPackedBytes": bridged_bytes, "AdditionalPackedBytes": bridged_bytes - source_bytes,
            "PackedExpansionNumerator": 18, "PackedExpansionDenominator": 17,
            "ArtifactBytesBeforeAlignmentChanges": estimated_file_bytes,
            "RawCodeCounts": code_counts, "FourthSymbolSupported": True,
            "TransformContract": required_transform,
            "SignIdentityCoverage": "inventory summaries only; actual sign bytes remain to be bound"}


def unique_object(pairs):
    """Refuse ambiguous duplicate JSON names in identity and geometry records."""
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field: " + key)
        result[key] = value
    return result


def load_json(path):
    raw = bounded_read(path, MAX_JSON_BYTES)
    # Python versions differ in JSON recursion limits; enforce our own bounded
    # structural depth, ignoring brackets and escaped quotes inside strings.
    depth = 0
    in_string = False
    escaped = False
    for byte in raw:
        if in_string:
            if escaped:
                escaped = False
            elif byte == 92:
                escaped = True
            elif byte == 34:
                in_string = False
        elif byte == 34:
            in_string = True
        elif byte in (91, 123):
            depth += 1
            require(depth <= MAX_JSON_DEPTH, "inventory exceeds JSON nesting limit")
        elif byte in (93, 125):
            depth -= 1
    def invalid_constant(value):
        raise ValueError("nonfinite JSON constant: " + value)
    try:
        record = json.loads(raw, object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except RecursionError:
        raise ValueError("inventory exceeds JSON nesting limit") from None
    require(type(record) is dict, "inventory must be a JSON object")
    return record, hashlib.sha256(raw).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[2]
    parser.add_argument("--metadata", type=Path, default=root / "config/swift.metadata.json")
    parser.add_argument("--census", type=Path, default=root / "config/swift.pq2-census.json")
    parser.add_argument("--payload", type=Path, help="raw PQ2 fixture, at most 1 MiB; never a GGUF model")
    parser.add_argument("--columns", type=int)
    parser.add_argument("--rows", type=int, default=1)
    args = parser.parse_args()
    try:
        require((args.payload is None) == (args.columns is None), "payload and columns must be supplied together")
        require(args.rows == 1 or args.payload is not None, "rows requires a payload")
        metadata, metadata_digest = load_json(args.metadata)
        census, census_digest = load_json(args.census)
        report = {"SchemaVersion": 1, "Qualification": "weight-layout-audit-only",
                  "SourceCommit": NATIVE_COMMIT,
                  "Sources": [SOURCE_BASE + "/ggml/src/ggml-common.h", SOURCE_BASE + "/ggml/src/ggml-quants.c"],
                  "MetadataSha256": metadata_digest, "CensusSha256": census_digest,
                  "StoragePlan": storage_plan(metadata, census), "Limitations": LIMITATIONS}
        if args.payload is not None:
            report["Fixture"] = audit_payload(bounded_read(args.payload, MAX_PAYLOAD_BYTES), args.columns, args.rows)
        print(json.dumps(report, indent=2, allow_nan=False))
    except (ValueError, OSError, KeyError, TypeError) as error:
        parser.exit(1, "PQ2 audit failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
