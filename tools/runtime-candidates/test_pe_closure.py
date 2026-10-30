"""Synthetic static PE fixtures; no binaries, DLLs or models are executed."""
from pathlib import Path
import struct
import tempfile
import unittest

from pe_closure import AuditError, PE, api_sets, audit


def image(path, normal=(), delayed=()):
    data = bytearray(0x1400)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<HH", data, 0x84, 0xAA64, 1)
    struct.pack_into("<H", data, 0x94, 240)
    struct.pack_into("<H", data, 0x98, 0x20B)
    struct.pack_into("<I", data, 0x98 + 108, 16)
    data[0x188:0x190] = b".rdata\0\0"
    struct.pack_into("<IIII", data, 0x190, 0x1000, 0x1000, 0x1000, 0x400)
    for names, directory, start, width, strings in [
        (normal, 1, 0x1000, 20, 0x1100),
        (delayed, 13, 0x1300, 32, 0x1500),
    ]:
        if names:
            struct.pack_into("<II", data, 0x98 + 112 + directory * 8, start, (len(names) + 1) * width)
        for index, name in enumerate(names):
            record = start - 0x1000 + 0x400 + index * width
            name_rva = strings + index * 80
            struct.pack_into("<I", data, record + (4 if directory == 13 else 12), name_rva)
            if directory == 13:
                struct.pack_into("<I", data, record, 1)
            offset = name_rva - 0x1000 + 0x400
            data[offset:offset + len(name) + 1] = name.encode("ascii") + b"\0"
    path.write_bytes(data)


def namespace_image(path):
    image(path)
    data = bytearray(path.read_bytes())
    data[0x188:0x190] = b".apiset\0"
    namespace = bytearray(200)
    name = "api-ms-win-core-test-l1-1-2".encode("utf-16le")
    host = "kernel32.dll".encode("utf-16le")
    struct.pack_into("<7I", namespace, 0, 6, 200, 0, 1, 28, 0, 31)
    struct.pack_into("<6I", namespace, 28, 0, 52, len(name), len(name) - 4, 120, 1)
    namespace[52:52 + len(name)] = name
    struct.pack_into("<5I", namespace, 120, 0, 0, 0, 140, len(host))
    namespace[140:140 + len(host)] = host
    data[0x400:0x400 + len(namespace)] = namespace
    path.write_bytes(data)


class ClosureTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        root = Path(self.scratch.name)
        self.bundle, self.system = root / "bundle", root / "system"
        self.bundle.mkdir()
        self.system.mkdir()
        image(self.bundle / "candidate.exe", ("common.dll",))
        image(self.bundle / "common.dll", ("kernel32.dll",))
        image(self.system / "kernel32.dll")

    def tearDown(self):
        self.scratch.cleanup()

    def test_candidate_normal_and_delay_gui_imports_reject(self):
        for delayed in (False, True):
            with self.subTest(delayed=delayed):
                image(self.bundle / "common.dll", () if delayed else ("shell32.dll",),
                      ("user32.dll",) if delayed else ())
                with self.assertRaisesRegex(AuditError, "GUI dependency"):
                    audit(self.bundle, self.system, {})

    def test_mandatory_system_gui_chain_rejects(self):
        image(self.system / "kernel32.dll", ("gdi32.dll",))
        with self.assertRaisesRegex(AuditError, "GUI dependency"):
            audit(self.bundle, self.system, {})

    def test_optional_os_delay_is_recorded_without_being_called_absent(self):
        image(self.system / "kernel32.dll", (), ("user32.dll",))
        report = audit(self.bundle, self.system, {})
        kernel = next(module for module in report["modules"] if module["name"] == "kernel32.dll")
        self.assertEqual(kernel["delayImports"], ["user32.dll"])
        self.assertIn("optional OS delays are recorded", report["scope"])
        self.assertFalse(report["modelExecuted"])

    def test_api_set_alias_resolves_and_unresolved_host_fails_closed(self):
        image(self.bundle / "common.dll", ("api-ms-win-core-test-l1-1-0.dll",))
        name = "api-ms-win-core-test-l1-1-0.dll"
        report = audit(self.bundle, self.system, {name: [("", "kernel32.dll")]})
        self.assertTrue(any(edge["import"] == name and edge["resolved"] == "kernel32.dll"
                            for edge in report["requiredEdges"]))
        report = audit(self.bundle, self.system,
                       {"api-ms-win-core-test-l1-1-*.dll": [("", "kernel32.dll")]})
        self.assertTrue(any(edge["import"] == name and edge["resolved"] == "kernel32.dll"
                            for edge in report["requiredEdges"]))
        with self.assertRaisesRegex(AuditError, "Unresolved API-set"):
            audit(self.bundle, self.system, {name: []})
        with self.assertRaisesRegex(AuditError, "GUI dependency"):
            audit(self.bundle, self.system, {name: [("", "user32.dll")]})

    def test_missing_and_malformed_dependencies_cannot_pass(self):
        (self.system / "kernel32.dll").unlink()
        with self.assertRaises((AuditError, OSError)):
            audit(self.bundle, self.system, {})
        (self.system / "kernel32.dll").write_bytes(b"MZ")
        with self.assertRaises(AuditError):
            PE(self.system / "kernel32.dll")
        image(self.system / "kernel32.dll")
        data = bytearray((self.bundle / "common.dll").read_bytes())
        struct.pack_into("<II", data, 0x98 + 120, 0x1000, 20)
        (self.bundle / "common.dll").write_bytes(data)
        with self.assertRaisesRegex(AuditError, "Unterminated import"):
            audit(self.bundle, self.system, {})

    def test_declared_optional_header_and_directory_count_are_enforced(self):
        target = self.bundle / "common.dll"
        for offset, format_, value in [
            (0x94, "<H", 111),
            (0x94, "<H", 224),
            (0x98 + 108, "<I", 1),
            (0x98 + 108, "<I", 17),
        ]:
            with self.subTest(offset=offset, value=value):
                image(target, ("kernel32.dll",))
                data = bytearray(target.read_bytes())
                struct.pack_into(format_, data, offset, value)
                target.write_bytes(data)
                with self.assertRaisesRegex(AuditError, "optional header|directory count"):
                    PE(target)

    def test_entire_normal_and_delay_directory_must_fit_its_raw_section(self):
        target = self.bundle / "common.dll"
        for directory, size in [(1, 40), (13, 64)]:
            with self.subTest(directory=directory):
                image(target)
                data = bytearray(target.read_bytes())
                # There is readable file padding beyond the section. It must
                # not make a directory crossing its declared raw end valid.
                data.extend(bytes(4096))
                struct.pack_into("<II", data, 0x98 + 112 + directory * 8, 0x1FF0, size)
                target.write_bytes(data)
                with self.assertRaisesRegex(AuditError, "extent outside raw section"):
                    PE(target).imports(delayed=directory == 13)

    def test_api_namespace_declared_size_bounds_entries_hosts_and_strings(self):
        target = self.system / "apisetschema.dll"
        namespace_image(target)
        mapping = api_sets(self.system)
        self.assertEqual(mapping["api-ms-win-core-test-l1-1-2.dll"], [("", "kernel32.dll")])
        for offset, value in [(4, 27), (4, 40), (16, 190), (28 + 4, 180),
                              (28 + 16, 190), (120 + 12, 184)]:
            with self.subTest(offset=offset, value=value):
                namespace_image(target)
                data = bytearray(target.read_bytes())
                struct.pack_into("<I", data, 0x400 + offset, value)
                target.write_bytes(data)
                with self.assertRaises(AuditError):
                    api_sets(self.system)


if __name__ == "__main__":
    unittest.main()
