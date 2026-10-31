"""Model-free byte/layout and inventory guards; no accelerator timing."""

import copy
import importlib.util
import json
from pathlib import Path
import random
import struct
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location("pq2_bridge", Path(__file__).with_name("pq2_bridge.py"))
BRIDGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BRIDGE)
ROOT = Path(__file__).resolve().parents[2]


class BridgeTests(unittest.TestCase):
    def test_known_byte_fixture_splits_at_element_64_and_keeps_fourth_symbol(self):
        # 0xe4 means -1,0,+1,+2; 0x1b reverses that order.
        source = bytes.fromhex("003c") + bytes([0xE4]) * 16 + bytes([0x1B]) * 16
        expected = bytes.fromhex("003c") + bytes([0xE4]) * 16 + bytes.fromhex("003c") + bytes([0x1B]) * 16
        self.assertEqual(BRIDGE.bridge_pq2(source, 128), expected)
        expected_values = (-1.0, 0.0, 1.0, 2.0) * 16 + (2.0, 1.0, 0.0, -1.0) * 16
        self.assertEqual(BRIDGE.decode_pq2_bits(source), struct.pack("<128f", *expected_values))
        self.assertEqual(BRIDGE.decode_q2_bits(expected), struct.pack("<128f", *expected_values))
        self.assertEqual(BRIDGE.audit_payload(source, 128)["RawCodeCounts"], [32] * 4)

    def test_every_packed_byte_and_lane_uses_native_little_endian_order(self):
        for packed in range(256):
            source = bytes.fromhex("0038") + bytes([packed]) * 32
            actual = BRIDGE.decode_q2_bits(BRIDGE.bridge_pq2(source, 128))
            expected = tuple((((packed >> shift) & 3) - 1) * 0.5 for shift in (0, 2, 4, 6)) * 32
            self.assertEqual(actual, struct.pack("<128f", *expected))

    def test_extreme_signed_subnormal_and_zero_scales_keep_raw_bits(self):
        for bits in (0x0000, 0x8000, 0x0001, 0x8001, 0x03FF, 0x83FF,
                     0x0400, 0x8400, 0x3C00, 0xBC00, 0x7BFF, 0xFBFF):
            source = bits.to_bytes(2, "little") + bytes([0xE4]) * 32
            bridged = BRIDGE.bridge_pq2(source, 128)
            self.assertEqual(bridged[:2], source[:2])
            self.assertEqual(bridged[18:20], source[:2])
            self.assertEqual(BRIDGE.decode_pq2_bits(source), BRIDGE.decode_q2_bits(bridged))
            self.assertEqual(BRIDGE.restore_pq2(bridged, 128), source)

    def test_unaligned_multirow_and_odd_group_payload_is_exact(self):
        generator = random.Random(0x20261030)
        for columns, rows in ((128, 1), (256, 2), (384, 3), (640, 1), (5120, 2)):
            blocks = columns // 128 * rows
            source = b"".join((0x3C00 + block).to_bytes(2, "little")
                              + generator.randbytes(32) for block in range(blocks))
            # memoryview begins at an intentionally odd byte offset.
            unaligned = memoryview(b"x" + source + b"y")[1:-1]
            result = BRIDGE.audit_payload(unaligned, columns, rows)
            self.assertEqual(result["BridgedBytes"], blocks * 36)
            self.assertEqual(result["DecodedF32Bits"], "exact")

    def test_nonfinite_scale_is_rejected_even_for_all_zero_codes(self):
        for bits in (0x7C00, 0xFC00, 0x7C01, 0x7FFF, 0xFC01, 0xFFFF):
            source = bits.to_bytes(2, "little") + bytes([0x55]) * 32
            with self.assertRaisesRegex(ValueError, "nonfinite"):
                BRIDGE.bridge_pq2(source, 128)

    def test_partial_rows_tails_and_invalid_dimensions_are_rejected(self):
        source = bytes.fromhex("003c") + bytes(32)
        for data, columns, rows in ((source[:-1], 128, 1), (source + b"x", 128, 1),
                                    (source, 64, 1), (source, 129, 1), (source, 128, 2),
                                    (b"", 0, 1), (source, 128, 0), (source, True, 1)):
            with self.assertRaises(ValueError):
                BRIDGE.bridge_pq2(data, columns, rows)
        with self.assertRaises(ValueError):
            BRIDGE.bridge_pq2(bytes(BRIDGE.MAX_PAYLOAD_BYTES + 1), 128)

    def test_inverse_rejects_unequal_scale_bits_including_signed_zero(self):
        bridged = bytearray(BRIDGE.bridge_pq2(bytes(34), 128))
        bridged[19] = 0x80
        with self.assertRaisesRegex(ValueError, "raw scale bits"):
            BRIDGE.restore_pq2(bridged, 128)
        with self.assertRaises(ValueError):
            BRIDGE.restore_pq2(bridged[:-1], 128)

    def test_retained_inventory_storage_cost_is_exact(self):
        metadata = json.loads((ROOT / "config/swift.metadata.json").read_bytes())
        census = json.loads((ROOT / "config/swift.pq2-census.json").read_bytes())
        report = BRIDGE.storage_plan(metadata, census)
        self.assertEqual(report["SourcePackedBytes"], 7_137_280_000)
        self.assertEqual(report["BridgedPackedBytes"], 7_557_120_000)
        self.assertEqual(report["AdditionalPackedBytes"], 419_840_000)
        self.assertEqual(report["ArtifactBytesBeforeAlignmentChanges"], 7_626_008_928)
        self.assertTrue(report["InventoryOnly"])

    def test_changed_identity_geometry_and_transform_fail_closed(self):
        metadata = json.loads((ROOT / "config/swift.metadata.json").read_bytes())
        census = json.loads((ROOT / "config/swift.pq2-census.json").read_bytes())
        for field, value in (("ArtifactSha256", "0" * 64), ("Blocks", 1),
                             ("CodePlusTwo", 1), ("NonfiniteScaleBlocks", 1),
                             ("TensorCount", False), ("SerializedBytesRead", -1),
                             ("CodecCommit", "0" * 40)):
            changed = dict(census, **{field: value})
            with self.assertRaises(ValueError):
                BRIDGE.storage_plan(metadata, changed)
        changed = copy.deepcopy(metadata)
        changed["Scalars"].pop("prism.hadamard.gdn_v_grouped")
        with self.assertRaisesRegex(ValueError, "transform"):
            BRIDGE.storage_plan(changed, census)
        changed = dict(metadata, TensorTypeHistogram=["invalid"])
        with self.assertRaises(ValueError):
            BRIDGE.storage_plan(changed, census)

    def test_duplicate_identity_fields_are_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate JSON"):
            json.loads('{"ArtifactSha256":"a","ArtifactSha256":"b"}',
                       object_pairs_hook=BRIDGE.unique_object)

    def test_inventory_validation_rejects_ambiguous_types_and_unbounded_extents(self):
        metadata = json.loads((ROOT / "config/swift.metadata.json").read_bytes())
        census = json.loads((ROOT / "config/swift.pq2-census.json").read_bytes())
        for name, identity in (("ModelRepository", ["ukisai", "model"]),
                               ("ModelRevision", ["revision"]), ("ModelRepository", "")):
            changed_metadata = dict(metadata, **{name: identity})
            changed_census = dict(census, **{name: identity})
            with self.assertRaisesRegex(ValueError, "string identity"):
                BRIDGE.storage_plan(changed_metadata, changed_census)
        for type_id in (142.0, "142", True):
            changed = copy.deepcopy(metadata)
            changed["TensorTypeHistogram"][2]["TypeId"] = type_id
            with self.assertRaisesRegex(ValueError, "tensor type"):
                BRIDGE.storage_plan(changed, census)
        changed = copy.deepcopy(metadata)
        changed["TensorTypeHistogram"][2]["Count"] = 0
        with self.assertRaisesRegex(ValueError, "positive PQ2 tensor count"):
            BRIDGE.storage_plan(changed, dict(census, TensorCount=0))
        for name in ("Blocks", "Elements", "TensorCount", "SerializedBytesRead"):
            with self.assertRaisesRegex(ValueError, "integer field"):
                BRIDGE.storage_plan(metadata, dict(census, **{name: 1 << 64}))
        changed = dict(metadata, FileBytes=BRIDGE.MAX_U64)
        with self.assertRaisesRegex(ValueError, "bridged storage"):
            BRIDGE.storage_plan(changed, dict(census, ArtifactBytes=BRIDGE.MAX_U64))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "inventory.json"
            quoted_brackets = {"text": "[]{}" * 256 + '"\\'}
            path.write_text(json.dumps(quoted_brackets), encoding="utf-8")
            self.assertEqual(BRIDGE.load_json(path)[0], quoted_brackets)
            for constant in ("NaN", "Infinity", "-Infinity"):
                path.write_text('{"unused":' + constant + '}', encoding="utf-8")
                with self.assertRaisesRegex(ValueError, "nonfinite JSON"):
                    BRIDGE.load_json(path)
            path.write_text('{"nested":' + "[" * 5000 + "0" + "]" * 5000 + "}", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "JSON nesting"):
                BRIDGE.load_json(path)


if __name__ == "__main__":
    unittest.main()
