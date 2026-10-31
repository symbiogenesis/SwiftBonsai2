"""Hosted-only public QAIC source build and independent IDL generation control.

The SDK executable, SDK imports and device runtime are never used. Cabal resolves
against one fixed Hackage index state, freezes both upstream projects, then the
reviewed upstream Makefile builds the generator. Only public pinned IDLs enter it.
"""

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import signal
import subprocess
import time
import urllib.request
from pathlib import Path

from public_qaic_wire import capture_wire, record_ghc_runtime
from public_idl_rust import compare_rust_wire


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def identity(path, name=None):
    if not path.is_file() or path.is_symlink():
        raise ValueError("ordinary retained provenance file required")
    return {"name": name or path.name, "bytes": path.stat().st_size, "sha256": digest(path)}


def write_json(path, value):
    data = (json.dumps(value, indent=2, ensure_ascii=True) + "\n").encode("ascii")
    if len(data) > 2 * 1024 * 1024:
        raise ValueError("public QAIC receipt exceeds its byte bound")
    temporary = path.with_suffix(path.suffix + ".partial")
    with temporary.open("wb") as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)


class Control:
    def __init__(self, review, env):
        self.review = review
        self.env = env
        self.deadline = time.monotonic() + 660
        self.commands = []

    def run(self, name, args, cwd, seconds=30):
        allowance = min(seconds, self.deadline - time.monotonic() - 15)
        if allowance <= 0:
            raise TimeoutError("aggregate hosted source-control budget exhausted")
        log = self.review / (name + ".log")
        started = time.monotonic()
        with log.open("wb") as output:
            process = subprocess.Popen(args, cwd=cwd, env=self.env, stdin=subprocess.DEVNULL,
                                       stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                while process.poll() is None:
                    if time.monotonic() - started > allowance or output.tell() > 4 * 1024 * 1024:
                        raise TimeoutError("bounded hosted source command exceeded its time/output allowance")
                    time.sleep(0.05)
                code = process.returncode
            finally:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
        record = {"name": name, "arguments": [str(value) for value in args], "exitCode": code,
                  "elapsedMilliseconds": int((time.monotonic() - started) * 1000), "log": identity(log)}
        self.commands.append(record)
        if code != 0:
            raise RuntimeError("public QAIC command failed: " + name)
        return log.read_text(encoding="utf-8", errors="replace")


def ordinary_tree(root):
    if not root.is_absolute():
        raise ValueError("expanded absolute source/output paths required")
    for ancestor in [root, *root.parents]:
        if ancestor.is_symlink():
            raise ValueError("linked source/output paths are unsupported")


def receive(entry, directory):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,127}", entry["name"]):
        raise ValueError("invalid public source filename")
    if not entry["url"].startswith("https://raw.githubusercontent.com/") or not 0 < entry["bytes"] <= 65625:
        raise ValueError("invalid bounded public source download")
    with urllib.request.urlopen(entry["url"], timeout=30) as response:
        if not response.geturl().startswith("https://raw.githubusercontent.com/"):
            raise ValueError("public source download left the reviewed HTTPS origin")
        data = response.read(entry["bytes"] + 1)
    if len(data) != entry["bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
        raise ValueError("public source input differs from its exact pin")
    path = directory / entry["name"]
    with path.open("xb") as output:
        output.write(data)
    return {**identity(path), "url": entry["url"]}


def retain_project(directory, name, review):
    retained = []
    for filename in ("cabal.project", "cabal.project.freeze", "cabal.project.local", "dist-newstyle/cache/plan.json"):
        path = directory / filename
        if not path.is_file():
            if filename in {"cabal.project", "cabal.project.freeze", "dist-newstyle/cache/plan.json"}:
                raise ValueError("source build omitted its exact dependency project/freeze/plan")
            continue
        if path.stat().st_size > 2 * 1024 * 1024:
            raise ValueError("dependency project/freeze/plan exceeds its bound")
        target = review / (name + "-" + filename.replace("/", "-"))
        shutil.copyfile(path, target)
        retained.append(identity(target))
    return retained


def run(args):
    if platform.system() != "Linux" or platform.machine() != "x86_64" or os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise ValueError("this control requires hosted Linux x86_64; owner fallback is unsupported")
    for name, pattern in (("GITHUB_SHA", r"[a-f0-9]{40}"), ("GITHUB_RUN_ID", r"[0-9]{1,32}"), ("GITHUB_RUN_ATTEMPT", r"[0-9]{1,8}")):
        if not re.fullmatch(pattern, os.environ.get(name, "")):
            raise ValueError("exact hosted source/run identity required")
    ordinary_tree(args.source)
    ordinary_tree(args.output)
    if args.output.exists() or not args.output.parent.is_dir():
        raise ValueError("new external output directory required")
    if any((ancestor / ".git").exists() for ancestor in args.output.parents):
        raise ValueError("build/evidence output must be outside Git storage")
    pin = json.loads(args.pin.read_text(encoding="utf-8-sig"))
    if pin["schemaVersion"] != 1 or pin["deviceInvocationAuthorized"] or pin["profileEligible"]:
        raise ValueError("unsupported public source-control authority")
    args.output.mkdir()
    review = args.output / "review"
    review.mkdir()
    inputs = review / "inputs-and-notices"
    inputs.mkdir()
    generated = args.output / "generated"
    generated.mkdir()
    env = dict(os.environ)
    for name in list(env):
        if name in {"GITHUB_TOKEN", "GH_TOKEN", "ACTIONS_RUNTIME_TOKEN", "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
                    "ACTIONS_ID_TOKEN_REQUEST_URL", "GHC_PACKAGE_PATH", "GHC_ENVIRONMENT", "GHC_OPTIONS",
                    "CABAL_CONFIG", "CABAL_DIR", "CABAL_BUILDDIR"}:
            env.pop(name, None)
        if name in {"LD_PRELOAD", "LD_LIBRARY_PATH", "GCC_EXEC_PREFIX", "COMPILER_PATH", "LIBRARY_PATH",
                    "CPATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "CFLAGS", "CPPFLAGS", "LDFLAGS"}:
            env.pop(name, None)
    cabal_dir = args.output / "cabal"
    cabal_dir.mkdir()
    config = cabal_dir / "config"
    config.write_text("repository hackage.haskell.org\n  url: https://hackage.haskell.org/\n  secure: True\n"
                      f"remote-repo-cache: {cabal_dir / 'packages'}\nstore-dir: {cabal_dir / 'store'}\n", encoding="utf-8")
    env.update({"CABAL_DIR": str(cabal_dir), "CABAL_CONFIG": str(config), "GHC_ENVIRONMENT": "-"})
    control = Control(review, env)
    report = {
        "schemaVersion": 1, "kind": "public-QAIC-independent-source-oracle",
        "runnerRevision": os.environ["GITHUB_SHA"], "runId": os.environ["GITHUB_RUN_ID"],
        "runAttempt": os.environ["GITHUB_RUN_ATTEMPT"], "qaicCommit": pin["qaicCommit"],
        "sourceSha256": digest(Path(__file__)), "pinSha256": digest(args.pin),
        "outcome": "pending", "stage": "source-identity", "generatorExecuted": False,
        "generatorAttempted": False, "generatorCompleted": False,
        "versionQueryAttempted": False, "versionQueryCompleted": False,
        "idlGenerationAttempted": False, "idlGenerationCompleted": False,
        "hackageIndexState": pin["hackageIndexState"], "sourceFiles": [], "publicInputs": [],
        "toolchain": {}, "dependencyResolution": [], "generatedFiles": [], "commands": control.commands,
        "wireCapture": None,
        "sdkExecutableUsed": False, "sdkImportsUsed": False,
        "sdkQaicEquivalenceQualified": False, "sdkImportEquivalenceQualified": False,
        "deviceCodeLoaded": False, "deviceInvocationAuthorized": False, "profileEligible": False,
    }
    try:
        root = control.run("source-root", ["git", "-C", str(args.source), "rev-parse", "--show-toplevel"], args.output).strip()
        revision = control.run("source-revision", ["git", "-C", str(args.source), "rev-parse", "HEAD"], args.output).strip()
        status = control.run("source-clean", ["git", "-C", str(args.source), "status", "--porcelain"], args.output).strip()
        if Path(root).resolve() != args.source.resolve() or revision != pin["qaicCommit"] or status:
            raise ValueError("QAIC checkout differs from the exact clean reviewed source")
        for entry in pin["sourceFiles"]:
            path = args.source / entry["path"]
            actual = identity(path, entry["path"])
            if actual["bytes"] != entry["bytes"] or actual["sha256"] != entry["sha256"]:
                raise ValueError("reviewed QAIC source/template differs from its pin")
            report["sourceFiles"].append(actual)
        tracked = control.run("source-inventory", ["git", "-C", str(args.source), "ls-files"], args.output).splitlines()
        if len(tracked) > 4096:
            raise ValueError("QAIC tracked source inventory exceeds its bound")
        full_source = []
        source_bytes = 0
        for name in tracked:
            path = args.source / name
            item = identity(path, name)
            source_bytes += item["bytes"]
            if source_bytes > 64 * 1024 * 1024:
                raise ValueError("QAIC tracked source bytes exceed their bound")
            full_source.append(item)
        write_json(review / "qaic-source-files.json", full_source)
        shutil.copyfile(args.source / "LICENSE.txt", inputs / "LICENSE-QAIC.txt")
        for name in ("CStub.c", "Stub.c", "Env.c", "Allocator.c", "Slim.c"):
            shutil.copyfile(args.source / "src/Language/SO" / name, inputs / ("QAIC-template-" + name))
        for entry in pin["publicInputs"]:
            report["publicInputs"].append(receive(entry, inputs))
        report["stage"] = "toolchain-identity"
        ghc = Path(shutil.which("ghc") or "").resolve()
        cabal = Path(shutil.which("cabal") or "").resolve()
        ghc_version = control.run("ghc-version", [str(ghc), "--numeric-version"], args.output).strip()
        cabal_version = control.run("cabal-version", [str(cabal), "--numeric-version"], args.output).strip()
        if ghc_version != pin["ghcVersion"] or cabal_version != pin["cabalVersion"]:
            raise ValueError("GHC/Cabal identity differs from the documented pinned versions")
        report["toolchain"] = {"ghcVersion": ghc_version, "cabalVersion": cabal_version,
                               "ghcExecutable": identity(ghc), "cabalExecutable": identity(cabal),
                               "setupAction": pin["setupAction"]}
        control.run("ghc-info", [str(ghc), "--info"], args.output)
        control.run("ghc-package-db", ["ghc-pkg", "dump"], args.output)
        report["toolchain"]["selectedGhcRuntime"] = record_ghc_runtime(ghc, control, args.output, identity)
        source = args.source / "src"
        helper = source / "file2hs"
        project = f"packages: .\nindex-state: {pin['hackageIndexState']}\nactive-repositories: hackage.haskell.org\n"
        for directory in (source, helper):
            if any((directory / name).exists() for name in ("cabal.project", "cabal.project.freeze", "cabal.project.local")):
                raise ValueError("upstream source unexpectedly contains a project override")
            (directory / "cabal.project").write_text(project, encoding="utf-8")
        report["stage"] = "pinned-dependency-resolution"
        control.run("hackage-update", [str(cabal), "update"], source, seconds=150)
        for name, directory in (("file2hs", helper), ("qaic", source)):
            control.run(name + "-freeze", [str(cabal), "freeze"], directory, seconds=60)
            freeze = directory / "cabal.project.freeze"
            if not freeze.is_file() or freeze.stat().st_size > 65536:
                raise ValueError("Cabal omitted its bounded exact dependency freeze")
            selected = pin["dependencyFreezePins"][name]
            if freeze.stat().st_size != selected["bytes"] or digest(freeze) != selected["sha256"]:
                raise ValueError("resolved dependency freeze differs from the reviewed first source control")
            shutil.copyfile(freeze, review / (name + "-before-build.freeze"))
        report["stage"] = "source-generator-build"
        control.run("upstream-make", pin["buildArguments"], args.source, seconds=420)
        report["dependencyResolution"] = retain_project(helper, "file2hs", review) + retain_project(source, "qaic", review)
        plan = json.loads((source / "dist-newstyle/cache/plan.json").read_bytes())
        external = [item for item in plan["install-plan"] if item.get("pkg-src", {}).get("type") == "repo-tar"]
        if len(external) != 1 or any(external[0].get(name) != value for name, value in pin["externalPackagePin"].items()):
            raise ValueError("external source dependency differs from the reviewed public source control")
        report["externalPackage"] = pin["externalPackagePin"]
        for name, directory in (("file2hs", helper), ("qaic", source)):
            if digest(directory / "cabal.project.freeze") != digest(review / (name + "-before-build.freeze")):
                raise ValueError("upstream build changed its preselected dependency freeze")
        executable = source / "Linux_ReleaseG/ship/qaic"
        report["generatorExecutable"] = identity(executable)
        report["generatorAttempted"] = True
        report["generatorExecuted"] = None
        report["versionQueryAttempted"] = True
        control.run("source-generator-version", [str(executable), "--version"], args.output)
        report["generatorExecuted"] = True
        report["versionQueryCompleted"] = True
        report["stage"] = "independent-IDL-generation"
        arguments = [str(executable), "--map-dll", "--standard-types", "--include-path=" + str(inputs),
                     "--output-path=" + str(generated), str(inputs / "htp_iface.idl")]
        report["idlGenerationAttempted"] = True
        control.run("independent-generation", arguments, args.output, seconds=15)
        report["generatorExecuted"] = True
        report["generatorCompleted"] = True
        report["idlGenerationCompleted"] = True
        files = list(generated.iterdir())
        if sorted(path.name for path in files) != sorted(pin["expectedGeneratedFiles"]):
            raise ValueError("public generator output set differs from the fixed IDL contract")
        retained = review / "generated-public-source"
        retained.mkdir()
        for path in files:
            if path.is_symlink() or path.stat().st_size > 1024 * 1024:
                raise ValueError("public generated output exceeds its per-file bound")
            data = path.read_bytes()
            item = identity(path)
            item["copyrightMarkerPresent"] = b"copyright" in data.lower()
            item["bsdClearIdentifierPresent"] = b"BSD-3-Clause-Clear" in data
            report["generatedFiles"].append(item)
            shutil.copyfile(path, retained / path.name)
        notices = ("Independent public-source QAIC output only.\n"
                   "QAIC compiler/templates: BSD-3-Clause-Clear; preserve LICENSE-QAIC.txt and template notices.\n"
                   "Public FastRPC IDL imports/runtime headers: BSD-3-Clause; preserve LICENSE-FastRPC.txt and original notices.\n"
                   "Target htp_iface.idl: llama.cpp MIT; preserve LICENSE-llama.cpp.txt and original input.\n"
                   "SDK6.7 generator/import equivalence and device invocation remain unqualified.\n")
        (review / "NOTICES.txt").write_text(notices, encoding="utf-8")
        report["stage"] = "independent-public-generation-complete"
        report["outcome"] = "generated-independent-oracle-only"
        if pin["generationReviewOnly"] is True:
            report["stage"] = "public-standard-type-generation-review"
            report["outcome"] = "public-standard-generated-source-awaiting-review"
            report["wireCapture"] = {"passed": False, "executionSelected": False,
                                     "reason": "new-standard-type-output-requires-independent-review"}
        elif pin["generationReviewOnly"] is False:
            capture_wire(pin, report, control, args.output, generated, review, receive, identity)
            compare_rust_wire(pin, report, control, args.output, review, args.rustc, identity)
        else:
            raise ValueError("the public generation review mode must be an explicit boolean")
    except Exception as error:
        report["outcome"] = "failed"
        report["failureClass"] = type(error).__name__
        report["failure"] = str(error)[:512]
        raise
    finally:
        report["commands"] = control.commands
        write_json(review / "host.json", report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--pin", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--rustc", required=True, type=Path)
    run(parser.parse_args())


if __name__ == "__main__":
    main()
