"""Named synthetic guards for exact native-state decoding and comparison."""
import importlib.util
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("native_compare", Path(__file__).with_name("compare.py"))
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def fixture(transposed):
    layout = MODULE.Layout(attention=(3,), recurrent=(0,), layers=4, width=2, convolution=3, matrix=4)
    data = bytearray()
    def add(code, *values):
        data.extend(struct.pack("<" + code, *values))
    add("II", 1, 2)
    for token in (0, 1):
        add("iIiii", token, 1, token, token, 0)
    add("II", transposed, 1)
    add("iQ", 1, 4)
    add("4e", 1.0, 2.0, 3.0, 4.0)
    if transposed:
        add("iII", 1, 2, 2)
        add("4e", 5.0, 7.0, 6.0, 8.0)
    else:
        add("iQ", 1, 4)
        add("4e", 5.0, 6.0, 7.0, 8.0)
    add("IiIII", 1, 1, 0, 0, 4)
    for values in ((9.0, 10.0, 11.0), (12.0, 13.0, 14.0, 15.0)):
        add("iQ", 0, len(values) * 4)
        add(str(len(values)) + "f", *values)
    return bytes(data), layout


class NativeStateTests(unittest.TestCase):
    def test_control_binding_rejects_changed_adapter_scale_tokens_and_backend(self):
        records = [{"event": "weights-loaded", "model_sha256": MODULE.MODEL,
                    "weight_representation": "current-repacked-PTQ1_0"},
                   {"event": "rank-one-adapter-loaded", "model_sha256": MODULE.MODEL,
                    "adapter_sha256": MODULE.ADAPTER, "target_pairs": 129, "scale_bits": "3fa66666"}]
        records += [{"event": "forward-step", "position": i, "input_token": token,
                     "backend_requested": "neon"} for i, token in enumerate((248045, 872))]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            self.assertEqual(len(MODULE.validate_rust_controls(path, {"adapterLoaded": True, "adapterScale": 1.3}, "neon")), 2)
            for capture, backend in (({"adapterLoaded": True, "adapterScale": 0}, "neon"),
                                     ({"adapterLoaded": False, "adapterScale": 0}, "neon"),
                                     ({"adapterLoaded": True, "adapterScale": 1.3}, "scalar")):
                with self.assertRaises(ValueError):
                    MODULE.validate_rust_controls(path, capture, backend)
            records[-1]["input_token"] = 198
            (path / "stdout.txt").write_text("\n".join(map(json.dumps, records)), encoding="utf-8")
            with self.assertRaises(ValueError):
                MODULE.validate_rust_controls(path, {"adapterLoaded": True, "adapterScale": 1.3}, "neon")

    def test_transposed_v_and_complete_recurrent_inventory_match_logical_rows(self):
        with tempfile.TemporaryDirectory() as directory:
            results = []
            for transposed in (0, 1):
                data, layout = fixture(transposed)
                path = Path(directory) / str(transposed)
                path.write_bytes(data)
                values = {name: [x[0] for x in struct.iter_unpack("<" + code, raw)]
                          for name, raw, code in MODULE.native_components(path, 1, layout)}
                self.assertEqual(values["block-3-value-0.f32le"], [5.0, 6.0])
                self.assertEqual(values["block-3-value-1.f32le"], [7.0, 8.0])
                self.assertEqual(values["block-0-convolution.f32le"], [9.0, 10.0, 11.0])
                self.assertEqual(values["block-0-recurrent.f32le"], [12.0, 13.0, 14.0, 15.0])
                self.assertEqual(len(values), 6)
                results.append(values)
            self.assertEqual(*results)

    def test_state_decoder_rejects_truncation_trailing_bytes_and_changed_schema(self):
        data, layout = fixture(1)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state"
            for bad in (data[:-1], data + b"extra", b"\x02\0\0\0" + data[4:]):
                path.write_bytes(bad)
                with self.assertRaises(ValueError):
                    list(MODULE.native_components(path, 1, layout))

    def test_comparator_enforces_finite_values_exact_extents_and_original_tolerance(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "actual"
            reference = struct.pack("2e", 1.0, -0.0)
            path.write_bytes(struct.pack("2f", 1.001, -0.0))
            self.assertEqual(MODULE.compare_payload(reference, "e", path)["failedElements"], 0)
            path.write_bytes(struct.pack("2f", 1.003, -0.0))
            self.assertEqual(MODULE.compare_payload(reference, "e", path)["failedElements"], 1)
            path.write_bytes(struct.pack("2f", float("nan"), 0.0))
            with self.assertRaises(ValueError):
                MODULE.compare_payload(reference, "e", path)
            path.write_bytes(struct.pack("f", 1.0))
            with self.assertRaises(ValueError):
                MODULE.compare_payload(reference, "e", path)


if __name__ == "__main__":
    unittest.main()
