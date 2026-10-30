"""Model-free controls for the fixed serial/parallel evidence comparator."""
import json
from pathlib import Path
import struct
import tempfile
import unittest

from compare import ADAPTER, GRAPH_IDENTITY, MODEL, Geometry, compare


class RowParallelTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.serial = self.root / "serial"
        self.parallel = self.root / "parallel"
        self.geometry = Geometry(layers=4, width=2, convolution=2, recurrent=3, vocabulary=8)
        # Independent miniature of three recurrent blocks and one attention block.
        files = {"logits-0.f32le": 8,
                 "state-0/block-0-convolution.f32le": 2, "state-0/block-0-recurrent.f32le": 3,
                 "state-0/block-1-convolution.f32le": 2, "state-0/block-1-recurrent.f32le": 3,
                 "state-0/block-2-convolution.f32le": 2, "state-0/block-2-recurrent.f32le": 3,
                 "state-0/block-3-key-0.f32le": 2, "state-0/block-3-value-0.f32le": 2}
        for directory, workers, operations, elapsed in ((self.serial, 1, 0, 200),
                                                       (self.parallel, 4, 6, 100)):
            directory.mkdir()
            records = [
                {"event": "weights-loaded", "model_sha256": MODEL, "weight_representation": "PQ2_0"},
                {"event": "rank-one-adapter-loaded", "adapter_sha256": ADAPTER,
                 "model_sha256": MODEL, "target_pairs": 129, "scale_bits": "3fa66666"},
                {"event": "forward-step", "position": 0, "input_token": 248045,
                 "backend_requested": "auto", "logit_count": 8, "row_workers": workers,
                 "parallel_row_operations": operations, "evaluation_milliseconds": elapsed,
                 "identity": GRAPH_IDENTITY, "greedy_token": 2, "checkpoint_bytes": 76,
                 "logits": str(directory / "logits-0.f32le")},
            ]
            self.write_records(directory, records)
            for relative, elements in files.items():
                path = directory / relative
                path.parent.mkdir(exist_ok=True)
                path.write_bytes(b"".join(struct.pack("<f", [1.0, -0.0, 2.0][index % 3])
                                         for index in range(elements)))

    @staticmethod
    def write_records(directory, records):
        (directory / "stdout.txt").write_text("\n".join(json.dumps(item) for item in records) + "\n",
                                             encoding="utf-8")

    def mutate(self, directory, operation):
        records = [json.loads(line) for line in (directory / "stdout.txt").read_text().splitlines()]
        operation(records)
        self.write_records(directory, records)

    def evaluate(self):
        return compare(self.serial, self.parallel, self.geometry)

    def test_complete_state_and_timing_observation_do_not_promote(self):
        report = self.evaluate()
        self.assertEqual((report["files"], report["elements"]), (9, 27))
        self.assertEqual(report["observedEvaluationSpeedup"], 2)
        self.assertFalse(report["performanceGatePassed"])
        self.assertFalse(report["defaultMayChange"])

    def test_same_wrong_extent_and_missing_component_on_both_sides_are_rejected(self):
        relative = "state-0/block-1-recurrent.f32le"
        for directory in (self.serial, self.parallel):
            (directory / relative).write_bytes(struct.pack("<f", 1))
        with self.assertRaisesRegex(ValueError, "extent"):
            self.evaluate()
        for directory in (self.serial, self.parallel):
            (directory / relative).unlink()
        with self.assertRaisesRegex(ValueError, "inventory"):
            self.evaluate()

    def test_equal_nonfinite_payloads_are_rejected(self):
        for directory in (self.serial, self.parallel):
            (directory / "state-0/block-3-key-0.f32le").write_bytes(struct.pack("<ff", float("nan"), 1))
        with self.assertRaisesRegex(ValueError, "nonfinite"):
            self.evaluate()

    def test_signed_zero_drift_and_unexpected_payload_are_rejected(self):
        path = self.parallel / "state-0/block-3-key-0.f32le"
        path.write_bytes(struct.pack("<ff", 1, 0.0))
        with self.assertRaisesRegex(ValueError, "bits differ"):
            self.evaluate()
        (self.parallel / "extra.f32le").write_bytes(struct.pack("<f", 1))
        with self.assertRaisesRegex(ValueError, "inventory"):
            self.evaluate()

    def test_requested_parallelism_must_have_executed(self):
        self.mutate(self.parallel, lambda records: records[-1].update(parallel_row_operations=0))
        with self.assertRaisesRegex(ValueError, "dispatch"):
            self.evaluate()

    def test_wrong_model_adapter_scale_tokens_or_worker_count_are_rejected(self):
        for index, key, value in ((0, "model_sha256", "0" * 64),
                                  (1, "scale_bits", "00000000"),
                                  (2, "input_token", 7), (2, "row_workers", 2), (2, "position", 0.0)):
            original = (self.parallel / "stdout.txt").read_text()
            with self.subTest(key=key):
                self.mutate(self.parallel, lambda records: records[index].update({key: value}))
                with self.assertRaises(ValueError):
                    self.evaluate()
            (self.parallel / "stdout.txt").write_text(original)

    def test_missing_duplicate_error_and_oversized_records_are_rejected(self):
        original = (self.parallel / "stdout.txt").read_text()
        for mutate in (lambda records: records.pop(), lambda records: records.append(records[-1]),
                       lambda records: records.append({"event": "error"})):
            self.mutate(self.parallel, mutate)
            with self.assertRaises(ValueError):
                self.evaluate()
            (self.parallel / "stdout.txt").write_text(original)
        (self.parallel / "stdout.txt").write_bytes(b" " * 65537)
        with self.assertRaisesRegex(ValueError, "byte bound"):
            self.evaluate()

    def test_timing_must_be_finite_positive_and_not_boolean(self):
        original = (self.parallel / "stdout.txt").read_text()
        for elapsed in (0, -1, True, 240001, float("inf"), float("nan")):
            with self.subTest(elapsed=elapsed):
                self.mutate(self.parallel, lambda records: records[-1].update(evaluation_milliseconds=elapsed))
                with self.assertRaisesRegex(ValueError, "duration"):
                    self.evaluate()
            (self.parallel / "stdout.txt").write_text(original)

    def test_matching_unbound_metadata_cannot_pass(self):
        originals = {directory: (directory / "stdout.txt").read_text()
                     for directory in (self.serial, self.parallel)}
        for key, value in (("identity", "wrong-unbound-graph"), ("logits", "unrelated.f32le"),
                           ("greedy_token", 999999), ("checkpoint_bytes", 0), ("greedy_token", 7)):
            with self.subTest(key=key, value=value):
                for directory in originals:
                    self.mutate(directory, lambda records: records[-1].update({key: value}))
                with self.assertRaises(ValueError):
                    self.evaluate()
                for directory, content in originals.items():
                    (directory / "stdout.txt").write_text(content)

    def test_nested_directories_and_unbounded_entries_are_rejected(self):
        nested = self.parallel / "state-0" / "nested"
        nested.mkdir()
        with self.assertRaisesRegex(ValueError, "nested"):
            self.evaluate()
        nested.rmdir()
        for index in range(33):
            (self.parallel / f"extra-{index}.txt").write_text("bounded fixture")
        with self.assertRaisesRegex(ValueError, "entry bound"):
            self.evaluate()


if __name__ == "__main__":
    unittest.main()
