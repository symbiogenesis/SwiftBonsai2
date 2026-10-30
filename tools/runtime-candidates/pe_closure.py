"""Bounded static ARM64 PE import audit; never loads a DLL or executes a target."""
import argparse
import hashlib
import json
from pathlib import Path
import struct

GUI = {"shell32.dll", "user32.dll", "gdi32.dll", "gdi32full.dll", "win32u.dll",
       "comdlg32.dll", "comctl32.dll", "dwmapi.dll", "uxtheme.dll", "imm32.dll"}
MAX_BYTES = 32 * 1024 * 1024
MAX_MODULES = 128


class AuditError(ValueError):
    """Unsafe, malformed or unresolved static dependency."""


class PE:
    def __init__(self, path):
        self.path = Path(path)
        if (self.path.is_symlink() or not self.path.is_file()
                or getattr(self.path.lstat(), "st_file_attributes", 0) & 0x400):
            raise AuditError("Dependency must be a regular file: " + self.path.name)
        with self.path.open("rb") as source:
            self.data = source.read(MAX_BYTES + 1)
        if len(self.data) > MAX_BYTES:
            raise AuditError("PE exceeds static read bound")
        if self.data[:2] != b"MZ":
            raise AuditError("Missing DOS signature")
        pe = self.u32(60)
        if self.slice(pe, 4) != b"PE\0\0" or self.u16(pe + 4) != 0xAA64:
            raise AuditError("Expected native ARM64 PE")
        self.optional = pe + 24
        optional_size = self.u16(pe + 20)
        if optional_size < 112:
            raise AuditError("Truncated PE32+ optional header")
        self.slice(self.optional, optional_size)
        if self.u16(self.optional) != 0x20B:
            raise AuditError("Expected PE32+")
        directory_count = self.u32(self.optional + 108)
        if directory_count < 14 or directory_count > (optional_size - 112) // 8:
            raise AuditError("PE directory count exceeds optional header or omits import tables")
        self.base = self.u64(self.optional + 24)
        self.sections = []
        section = self.optional + optional_size
        count = self.u16(pe + 6)
        if count > 96:
            raise AuditError("Too many PE sections")
        self.slice(section, count * 40)
        for index in range(count):
            header = section + index * 40
            values = struct.unpack("<IIII", self.slice(header + 8, 16))
            self.slice(values[3], values[2])
            self.sections.append((self.slice(header, 8).rstrip(b"\0"), *values))

    def slice(self, offset, size):
        if offset < 0 or size < 0 or offset + size > len(self.data):
            raise AuditError("PE extent outside file")
        return self.data[offset:offset + size]

    def u16(self, offset):
        return struct.unpack("<H", self.slice(offset, 2))[0]

    def u32(self, offset):
        return struct.unpack("<I", self.slice(offset, 4))[0]

    def u64(self, offset):
        return struct.unpack("<Q", self.slice(offset, 8))[0]

    def offset(self, rva):
        return self.extent(rva, 1)[0]

    def extent(self, rva, size):
        for _, virtual_size, virtual, raw_size, raw in self.sections:
            if virtual <= rva < virtual + max(virtual_size, raw_size):
                delta = rva - virtual
                if size < 0 or delta + size > raw_size:
                    raise AuditError("PE RVA extent outside raw section")
                self.slice(raw + delta, size)
                return raw + delta, raw_size - delta
        raise AuditError("Unmapped PE RVA")

    def module(self, rva):
        offset, available = self.extent(rva, 1)
        tail = self.slice(offset, min(260, available))
        end = tail.find(b"\0")
        if end < 1:
            raise AuditError("Unterminated import name")
        name = tail[:end].decode("ascii").lower()
        if not name.endswith(".dll") or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_." for c in name):
            raise AuditError("Unsafe import module name")
        return name

    def imports(self, delayed=False):
        directory = self.optional + 112 + (13 if delayed else 1) * 8
        rva, size = self.u32(directory), self.u32(directory + 4)
        if not rva and not size:
            return []
        width = 32 if delayed else 20
        if not rva or size < width or size > 65536:
            raise AuditError("Invalid import directory extent")
        start, _ = self.extent(rva, size)
        result = []
        for position in range(0, size - width + 1, width):
            values = struct.unpack("<" + "I" * (width // 4), self.slice(start + position, width))
            if not any(values):
                return result
            name = values[1] if delayed else values[3]
            if delayed and not values[0] & 1:
                name -= self.base
            result.append(self.module(name))
        raise AuditError("Unterminated import directory")

    def receipt(self):
        return {"name": self.path.name, "bytes": len(self.data),
                "sha256": hashlib.sha256(self.data).hexdigest(), "machine": "0xAA64",
                "normalImports": self.imports(), "delayImports": self.imports(True)}


def api_sets(system):
    """Read this Windows image's static API-set namespace, including aliases."""
    pe = PE(system / "apisetschema.dll")
    sections = [section for section in pe.sections if section[0] == b".apiset"]
    if len(sections) != 1:
        raise AuditError("Expected one API-set namespace")
    _, _, _, raw_size, raw = sections[0]
    namespace = pe.slice(raw, raw_size)

    def u32(offset):
        if offset < 0 or offset + 4 > len(namespace):
            raise AuditError("API-set extent outside namespace")
        return struct.unpack_from("<I", namespace, offset)[0]

    def text(offset, length):
        if length % 2 or offset < 0 or offset + length > len(namespace):
            raise AuditError("Invalid API-set name extent")
        return namespace[offset:offset + length].decode("utf-16le").lower()

    if len(namespace) < 28 or u32(0) != 6 or not 28 <= u32(4) <= len(namespace):
        raise AuditError("Unsupported API-set namespace")
    namespace = namespace[:u32(4)]
    entry_count, entry_offset = u32(12), u32(16)
    if entry_count > 4096 or entry_offset < 28 or entry_offset + entry_count * 24 > len(namespace):
        raise AuditError("API-set entry table outside declared namespace")
    result = {}
    for index in range(entry_count):
        entry = entry_offset + index * 24
        if not 0 < u32(entry + 12) <= u32(entry + 8):
            raise AuditError("Invalid API-set hashed name extent")
        name = text(u32(entry + 4), u32(entry + 8)) + ".dll"
        hashed = text(u32(entry + 4), u32(entry + 12)) + "-*.dll"
        count, values = u32(entry + 20), u32(entry + 16)
        if count > 64 or values + count * 20 > len(namespace):
            raise AuditError("API-set host table outside declared namespace")
        hosts = [(text(u32(values + n * 20 + 4), u32(values + n * 20 + 8)),
                  text(u32(values + n * 20 + 12), u32(values + n * 20 + 16))) for n in range(count)]
        result[name] = hosts
        if hashed in result and result[hashed] != hosts:
            raise AuditError("Ambiguous API-set hashed contract")
        result[hashed] = hosts
    return result


def audit(bundle, system, aliases=None):
    bundle, system = Path(bundle), Path(system)
    if (bundle.is_symlink() or system.is_symlink()
            or getattr(bundle.lstat(), "st_file_attributes", 0) & 0x400
            or getattr(system.lstat(), "st_file_attributes", 0) & 0x400):
        raise AuditError("Redirected audit directory")
    candidates = {p.name.lower(): p for p in bundle.iterdir() if p.suffix.lower() in {".exe", ".dll"}}
    if not candidates or len(candidates) > 64:
        raise AuditError("Candidate inventory outside bound")
    aliases = api_sets(system) if aliases is None else aliases
    pending = [(name, "candidate") for name in candidates]
    modules = {}
    edges = []
    while pending:
        name, location = pending.pop()
        key = (name, location)
        if key in modules:
            continue
        if name in GUI:
            raise AuditError("GUI dependency: " + name)
        if len(modules) >= MAX_MODULES:
            raise AuditError("Dependency closure exceeds module bound")
        path = candidates[name] if location == "candidate" else system / name
        metadata = PE(path).receipt()
        modules[key] = {**metadata, "location": location}
        # OS delay loads are not initialized dependencies. Record them rather
        # than claiming they are absent: core Windows DLLs expose optional GUI APIs.
        required = metadata["normalImports"] + (metadata["delayImports"] if location == "candidate" else [])
        for dependency in required:
            resolved = dependency
            # Namespace v6 hashes omit the final contract revision. Windows
            # can map an older imported revision to its newer installed host.
            hashed = dependency.rsplit("-", 1)[0] + "-*.dll"
            hosts = aliases.get(dependency, aliases.get(hashed))
            if hosts is not None:
                matches = [host for alias, host in hosts if alias in {name, name.removesuffix(".dll")}]
                matches = matches or [host for alias, host in hosts if not alias]
                if len(set(matches)) != 1 or not matches[0]:
                    raise AuditError("Unresolved API-set host: " + dependency)
                resolved = matches[0]
            if not resolved.endswith(".dll") or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_." for c in resolved):
                raise AuditError("Unsafe resolved module name")
            destination = "candidate" if resolved in candidates else "system"
            edges.append({"from": name, "import": dependency, "resolved": resolved,
                          "location": destination})
            pending.append((resolved, destination))
    return {"schemaVersion": 1, "status": "candidate-and-mandatory-named-import-closure-gui-free",
            "scope": "All candidate normal/delay imports and recursive system normal imports; optional OS delays are recorded, not followed or claimed absent. No DLL was loaded.",
            "forwardedExportsResolved": False,
            "modules": sorted(modules.values(), key=lambda item: (item["location"], item["name"])),
            "requiredEdges": edges, "modelExecuted": False, "workerPolicyChanged": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", required=True, type=Path)
    parser.add_argument("--system", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    options = parser.parse_args()
    try:
        result = audit(options.bundle, options.system)
    except (AuditError, OSError, UnicodeError, struct.error) as error:
        result = {"schemaVersion": 1, "status": "rejected", "error": str(error), "modelExecuted": False}
        options.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        raise SystemExit(1) from None
    options.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
