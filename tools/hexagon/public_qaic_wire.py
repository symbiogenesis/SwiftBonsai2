"""Compile and capture public generated marshalling against host-only RPC mocks."""

import json
import os
import re
import shutil
from pathlib import Path


def unique_object(pairs):
    result = {}
    for name, value in pairs:
        if name in result:
            raise ValueError("duplicate mock capture JSON key")
        result[name] = value
    return result


def equal_typed(actual, expected):
    if type(actual) is not type(expected):
        return False
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(
            equal_typed(actual[name], value) for name, value in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(
            equal_typed(left, right) for left, right in zip(actual, expected))
    return actual == expected


def record_ghc_runtime(ghc, control, output, identify):
    """Observe the selected installation, without claiming its full OS closure."""
    libdir = Path(control.run("ghc-libdir", [str(ghc), "--print-libdir"], output).strip()).resolve(strict=True)
    toolroot = ghc.parent.parent.resolve(strict=True)
    result = {"reportedLibdir": str(libdir), "selectedToolRoot": str(toolroot),
              "scope": "selected-installation-compiler/runtime-executable-candidates; not-complete-tool-or-OS-closure",
              "scanTruncated": False, "entriesObserved": 0, "candidates": []}
    if not libdir.is_relative_to(toolroot):
        result["scope"] = "reported-libdir-outside-selected-wrapper-installation; runtime-closure-pending"
        return result
    observed_paths = set()
    for directory, subdirectories, files in os.walk(toolroot, followlinks=False):
        subdirectories[:] = [name for name in subdirectories if not (Path(directory) / name).is_symlink()]
        result["entriesObserved"] += len(subdirectories) + len(files)
        if result["entriesObserved"] > 50000:
            result["scanTruncated"] = True
            break
        for name in files:
            if name not in {"ghc", ghc.name} and not name.startswith("ghc-iserv"):
                continue
            discovered = Path(directory) / name
            path = discovered.resolve(strict=True)
            if not path.is_relative_to(toolroot) or not path.is_file() or path in observed_paths:
                continue
            observed_paths.add(path)
            with path.open("rb") as source:
                signature = source.read(4)
            result["candidates"].append({**identify(path), "path": str(path), "discoveredAs": str(discovered),
                                          "elfExecutable": signature == b"\x7fELF"})
    return result


def capture_wire(pin, report, control, output, generated, review, receive, identify):
    wire_pin = pin["wireCapture"]
    if wire_pin["enabled"] is not True or wire_pin["sdkLinked"] is not False or \
            wire_pin["deviceInvocationAuthorized"] is not False or wire_pin["profileEligible"] is not False:
        raise ValueError("the fixed mock wire control must be explicitly selected")
    wire = {"kind": "public-generated-C-mock-wire-control", "passed": False,
            "controller": identify(Path(__file__)), "headers": [], "compiler": {},
            "mockExecutionAttempted": False, "mockExecuted": False, "mockCompleted": False,
            "sdkLinked": False, "driverLoaded": False, "deviceCodeLoaded": False,
            "sdkEquivalenceQualified": False, "WindowsArm64AbiQualified": False,
            "scope": "public-generated-stub-and-public-headers-on-hosted-Linux-LP64-only"}
    report["wireCapture"] = wire
    report["stage"] = "mock-wire-source-intake"
    inputs = review / "wire-public-inputs"
    inputs.mkdir()
    for entry in wire_pin["headers"]:
        wire["headers"].append(receive(entry, inputs))
    local = Path(__file__).parent
    for entry in (wire_pin["harness"], wire_pin["expected"]):
        path = local / entry["name"]
        actual = identify(path)
        if actual["bytes"] != entry["bytes"] or actual["sha256"] != entry["sha256"]:
            raise ValueError("wire harness or independent expectation differs from its source pin")
        shutil.copyfile(path, inputs / entry["name"])
    wire["harness"] = identify(inputs / wire_pin["harness"]["name"])
    wire["independentExpected"] = identify(inputs / wire_pin["expected"]["name"])
    expected = json.loads((inputs / wire_pin["expected"]["name"]).read_bytes(), object_pairs_hook=unique_object)
    for entry in wire_pin["reviewedGeneratedFiles"]:
        actual = identify(generated / entry["name"])
        if actual != entry:
            raise ValueError("public generated C differs from the reviewed first-generation bytes")
    wire["generatedInputs"] = [identify(generated / name) for name in ("htp_iface.h", "htp_iface_stub.c")]
    report["stage"] = "mock-wire-compiler-identity"
    compiler = Path("/usr/bin/gcc").resolve(strict=True)
    original = identify(compiler)
    version = control.run("wire-gcc-version", [str(compiler), "-dumpfullversion", "-dumpversion"], output).strip()
    target = control.run("wire-gcc-target", [str(compiler), "-dumpmachine"], output).strip()
    control.run("wire-gcc-banner", [str(compiler), "--version"], output)
    control.run("wire-gcc-specs", [str(compiler), "-dumpspecs"], output)
    if not re.fullmatch(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?", version) or target != "x86_64-linux-gnu":
        raise ValueError("mock wire control requires the fixed native Linux x86_64 C target")
    components = []
    for name in ("cc1", "as", "ld"):
        candidate = control.run("wire-gcc-" + name, [str(compiler), "-print-prog-name=" + name], output).strip()
        path = Path(candidate)
        if not path.is_absolute():
            resolved = shutil.which(candidate, path=control.env.get("PATH"))
            if not resolved:
                raise ValueError("GCC component could not be resolved for retained provenance")
            path = Path(resolved)
        path = path.resolve(strict=True)
        components.append({**identify(path), "path": str(path), "role": name})
    wire["compiler"] = {**original, "path": str(compiler), "version": version, "target": target,
                        "components": components, "identityPinScope": "observed-and-rechecked-within-this-hosted-run",
                        "closure": "driver/cc1/as/ld identities; not-complete-OS-closure"}
    flags = ["-std=gnu11", "-O0", "-fno-lto", "-fno-strict-aliasing"]
    if wire_pin["compilerFlags"] != flags:
        raise ValueError("the fixed mock-only compiler flags changed")
    executable = output / "public-qaic-wire-mock"
    link_map = review / "wire-link.map"
    arguments = [str(compiler), *flags, "-I" + str(inputs), "-I" + str(generated),
                 str(generated / "htp_iface_stub.c"), str(inputs / wire_pin["harness"]["name"]),
                 "-Wl,-Map=" + str(link_map), "-o", str(executable)]
    wire["compiler"]["arguments"] = arguments
    report["stage"] = "mock-wire-compile"
    control.run("wire-compile", arguments, output, seconds=30)
    if identify(compiler) != original:
        raise ValueError("GCC driver changed during the bounded mock compilation")
    for component in components:
        actual = identify(Path(component["path"]))
        if actual != {name: component[name] for name in ("name", "bytes", "sha256")}:
            raise ValueError("GCC component changed during the bounded mock compilation")
    if not link_map.is_file() or link_map.stat().st_size > 1024 * 1024:
        raise ValueError("mock link map omitted or exceeded its bound")
    wire["linkMap"] = identify(link_map)
    wire["executable"] = identify(executable)
    report["stage"] = "mock-wire-execution"
    wire["mockExecutionAttempted"] = True
    wire["mockExecuted"] = None
    try:
        text = control.run("wire-capture", [str(executable)], output, seconds=5)
    finally:
        if control.commands and control.commands[-1]["name"] == "wire-capture":
            wire["mockExecuted"] = True
            wire["mockCompleted"] = True
    if identify(executable) != wire["executable"]:
        raise ValueError("mock executable changed during the bounded capture")
    if len(text.encode("utf-8")) > 16384:
        raise ValueError("mock capture receipt exceeded its fixed output bound")
    actual = json.loads(text, object_pairs_hook=unique_object)
    if actual.keys() != {"schemaVersion", "kind", "testVectorId", "handleHex", "start", "stop",
                         "callCount", "unexpectedCalls", "returnCodes", "layout"}:
        raise ValueError("mock capture emitted an unsupported receipt shape")
    for name in ("schemaVersion", "testVectorId", "handleHex", "start", "stop"):
        if not equal_typed(actual[name], expected[name]):
            raise ValueError("public generated wire differs from the independent source-derived vector: " + name)
    layout = {"charBits": 8, "pointerBytes": 8, "sizeTBytes": 8, "uint32Bytes": 4,
              "uint64Bytes": 8, "remoteHandle64Bytes": 8, "remoteBufferBytes": 16,
              "remoteBufferAlignment": 8, "remoteBufferPointerOffset": 0, "remoteBufferLengthOffset": 8,
              "remoteArgBytes": 16, "remoteArgAlignment": 8, "littleEndian": True}
    checks = {"kind": "public-qaic-mock-wire-capture", "callCount": 2, "unexpectedCalls": 0,
              "returnCodes": {"start": 0, "stop": 0}, "layout": layout}
    if not all(equal_typed(actual[name], value) for name, value in checks.items()):
        raise ValueError("mock capture failed its fixed call/return/native-layout contract")
    wire["captureReceipt"] = identify(review / "wire-capture.log")
    wire["observed"] = actual
    wire["independentVectorMatched"] = True
    wire["compilerIdentityRechecked"] = True
    wire["passed"] = True
    report["stage"] = "independent-public-mock-wire-complete"
    report["outcome"] = "captured-independent-public-wire-only"
