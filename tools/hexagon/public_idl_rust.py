"""Compare a standalone Rust encoder with the independently captured C wire."""

import json
import shutil
from pathlib import Path

from public_qaic_wire import equal_typed, unique_object


def compare_rust_wire(pin, report, control, output, review, rustc_path, identify):
    selection = pin["rustWireControl"]
    if selection["deviceInvocationAuthorized"] is not False:
        raise ValueError("Rust wire encoding does not authorize device invocation")
    capture = report["wireCapture"]
    if capture["passed"] is not True:
        raise ValueError("the independent generated-C wire capture must pass first")
    compiler = rustc_path.resolve(strict=True)
    compiler_identity = identify(compiler)
    wire = {
        "kind": "independent-Rust-public-wire-encoding-control",
        "passed": False,
        "controller": identify(Path(__file__)),
        "compiler": compiler_identity,
        "testExecutionAttempted": False,
        "testExecutionCompleted": False,
        "encodingExecutionAttempted": False,
        "encodingExecutionCompleted": False,
        "driverLoaded": False,
        "deviceExecuted": False,
        "sdkCompatibilityQualified": False,
        "profileEligible": False,
    }
    report["rustWireControl"] = wire
    report["stage"] = "independent-Rust-wire-compiler"
    version = control.run("wire-rustc-version", [str(compiler), "-vV"], output)
    if "host: " + selection["target"] not in version.splitlines():
        raise ValueError("Rust wire control requires its fixed native Linux target")
    sysroot = Path(control.run("wire-rustc-sysroot", [str(compiler), "--print", "sysroot"], output).strip()).resolve(strict=True)
    if sysroot.name != selection["toolchain"] or compiler.parent.parent != sysroot:
        raise ValueError("Rust compiler resolved outside the selected dated toolchain")
    wire["toolchain"] = selection["toolchain"]
    wire["target"] = selection["target"]
    wire["sysroot"] = str(sysroot)
    wire["identityScope"] = "selected rustc/driver files and verbose identity; not-complete-OS-closure"
    drivers = list((sysroot / "lib").glob("librustc_driver-*.so"))
    if len(drivers) > 8:
        raise ValueError("Rust compiler driver identity count exceeds its bound")
    wire["selectedCompilerDrivers"] = []
    for path in drivers:
        if path.stat().st_size > 512 * 1024 * 1024:
            raise ValueError("Rust compiler driver identity exceeds its byte bound")
        wire["selectedCompilerDrivers"].append({**identify(path), "path": str(path)})

    source = Path(__file__).parent / selection["source"]["name"]
    original_source = identify(source)
    if original_source != selection["source"]:
        raise ValueError("the independent Rust encoder differs from its source pin")
    retained_source = review / source.name
    shutil.copyfile(source, retained_source)
    wire["source"] = identify(retained_source)
    linker = Path(capture["compiler"]["path"])
    if identify(linker) != {key: capture["compiler"][key] for key in ("name", "bytes", "sha256")}:
        raise ValueError("the reviewed C linker driver changed before Rust compilation")
    flags = ["--edition=2024", "--target", selection["target"], "-D", "warnings",
             "-C", "opt-level=2", "-C", "overflow-checks=yes", "-C", "linker=" + str(linker)]
    wire["compilerFlags"] = flags
    executable = output / "public-idl-rust-control"
    tests = output / "public-idl-rust-contract"
    report["stage"] = "independent-Rust-wire-build"
    control.run("wire-rust-build", [str(compiler), *flags, str(source), "-o", str(executable)], output, seconds=30)
    control.run("wire-rust-test-build", [str(compiler), *flags, "--test", str(source), "-o", str(tests)], output, seconds=30)
    if identify(compiler) != compiler_identity or identify(source) != original_source:
        raise ValueError("Rust compiler or encoder changed during its bounded build")
    wire["executable"] = identify(executable)
    wire["contractExecutable"] = identify(tests)
    report["stage"] = "independent-Rust-wire-test"
    wire["testExecutionAttempted"] = True
    control.run("wire-rust-contract", [str(tests)], output, seconds=5)
    wire["testExecutionCompleted"] = True
    report["stage"] = "independent-Rust-wire-encoding"
    wire["encodingExecutionAttempted"] = True
    text = control.run("wire-rust-encoding", [str(executable)], output, seconds=5)
    wire["encodingExecutionCompleted"] = True
    if identify(executable) != wire["executable"] or identify(tests) != wire["contractExecutable"]:
        raise ValueError("Rust control bytes changed during execution")
    if len(text.encode("utf-8")) > 16384:
        raise ValueError("Rust encoding receipt exceeds its byte bound")
    actual = json.loads(text, object_pairs_hook=unique_object)
    shared = ("schemaVersion", "testVectorId", "handleHex", "start", "stop")
    flags_false = ("driverLoaded", "deviceExecuted", "sdkCompatibilityQualified", "profileEligible")
    if actual.keys() != {*shared, *flags_false, "kind"} or actual["kind"] != "public-idl-rust-encoding-control":
        raise ValueError("Rust encoding emitted an unsupported receipt shape")
    if any(actual[name] is not False for name in flags_false):
        raise ValueError("Rust encoding cannot establish device or profile authority")
    if any(not equal_typed(actual[name], capture["observed"][name]) for name in shared):
        raise ValueError("Rust encoding differs from the independently captured generated-C wire")
    wire["observed"] = actual
    wire["captureReceipt"] = identify(review / "wire-rust-encoding.log")
    wire["generatedCWireMatched"] = True
    wire["passed"] = True
    report["stage"] = "independent-public-C-Rust-wire-matched"
    report["outcome"] = "public-generated-C-and-Rust-wire-cross-checked-only"
