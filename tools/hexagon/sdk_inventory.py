"""Read a pinned SDK archive as data; never extract or execute SDK contents.

The hosted supervisor retains a deny-write/delete handle for the verified archive
through this process's confirmed exit. This reader verifies the same open bytes
again, then streams bounded tar metadata and selected file contents into hashes.
"""

import argparse
import hashlib
import json
import re
import tarfile
from pathlib import Path, PurePosixPath


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def checked_name(value):
    if len(value) > 1024 or any(ord(char) < 32 for char in value) or "\\" in value:
        raise ValueError("unsupported archive entry name")
    name = PurePosixPath(value)
    if name.is_absolute() or ".." in name.parts or ":" in value:
        raise ValueError("archive entry escapes its relative namespace")
    return str(name)


def category(name, pin):
    lower = name.lower()
    leaf = PurePosixPath(lower).name
    if name == pin["intendedQaicPath"]:
        return "qaic-generator"
    if lower.endswith("/build/cmake/hexagon_fun.cmake"):
        return "build-idl-recipe"
    if lower.endswith(".idl") and (
        "/incs/" in lower or "/ipc/fastrpc/" in lower or leaf in {"remote.idl", "aeestddef.idl"}
    ):
        return "sdk-idl-input-candidate"
    if leaf in {"remote.h", "remote_wos_ext.h", "dspqueue.h", "rpcmem.h"}:
        return "sdk-rpc-header-candidate"
    if re.match(r"^(license|licence|eula|notice|copyright)([._-]|$)", leaf) and (
        len(PurePosixPath(name).parts) <= 3 or "/ipc/fastrpc/" in lower or "/build/" in lower
    ):
        return "license-review"
    return None


def review(data, kind, words):
    if kind not in {"build-idl-recipe", "license-review"}:
        return None
    try:
        text = data.decode("utf-8-sig")
    except UnicodeError:
        return {"textEncoding": "not-UTF8", "excerpt": None}
    if kind == "license-review":
        return {"textEncoding": "UTF8", "excerpt": " ".join(text.split()[:words])}
    lines = text.splitlines()
    qaic = [(index + 1, line.strip()) for index, line in enumerate(lines) if "qaic" in line.lower()]
    # Limited source excerpts help locate the exact recipe. Flag spellings are
    # functional metadata, not an interpreted or authorized generator command.
    excerpt = " ".join(" ".join(line for _, line in qaic).split()[:words])
    flags = sorted(set(re.findall(r"(?<![A-Za-z0-9_])-[A-Za-z][A-Za-z0-9_-]*", text)))
    return {
        "textEncoding": "UTF8",
        "qaicReferenceLines": [index for index, _ in qaic[:64]],
        "qaicReferenceExcerpt": excerpt,
        "flagSpellingsMentionedInRecipe": flags[:128],
        "commandAndImportInterpretationReviewed": False,
    }


def inventory(archive, pin):
    limits = pin["limits"]
    selected = []
    names = set()
    declared = selected_bytes = entries = links = 0
    with archive.open("rb") as source:
        if source.seek(0, 2) != pin["asset"]["bytes"]:
            raise ValueError("SDK archive extent differs from its pin")
        source.seek(0)
        if hashlib.file_digest(source, "sha256").hexdigest() != pin["asset"]["sha256"]:
            raise ValueError("SDK archive digest differs from its pin")
        source.seek(0)
        with tarfile.open(fileobj=source, mode="r|xz") as contents:
            for member in contents:
                entries += 1
                if entries > limits["archiveEntries"]:
                    raise ValueError("SDK archive entry count exceeds its bound")
                name = checked_name(member.name)
                # Directory entries may repeat their implied parent; files may not.
                if not member.isdir():
                    if name.casefold() in names:
                        raise ValueError("SDK archive contains duplicate file identities")
                    names.add(name.casefold())
                if member.size < 0:
                    raise ValueError("SDK archive contains a negative extent")
                declared += member.size
                if declared > limits["declaredUncompressedBytes"]:
                    raise ValueError("SDK declared uncompressed bytes exceed their bound")
                kind = category(name, pin)
                if member.issym() or member.islnk():
                    links += 1
                    if kind:
                        raise ValueError("selected SDK input is a link instead of original bytes")
                    continue
                if not kind or member.isdir():
                    continue
                if not member.isfile():
                    raise ValueError("selected SDK input is not an ordinary file")
                if member.size > limits["selectedFileBytes"] or len(selected) >= limits["selectedFiles"]:
                    raise ValueError("selected SDK input exceeds its per-file/count bound")
                selected_bytes += member.size
                if selected_bytes > limits["selectedBytes"]:
                    raise ValueError("selected SDK bytes exceed their aggregate bound")
                payload = contents.extractfile(member)
                if payload is None:
                    raise ValueError("selected SDK payload is unavailable")
                with payload:
                    data = payload.read(member.size + 1)
                if len(data) != member.size:
                    raise ValueError("selected SDK payload extent changed")
                entry = {"path": name, "category": kind, "bytes": member.size, "sha256": sha256(data)}
                excerpt = review(data, kind, limits["reviewWordsPerFile"])
                if excerpt is not None:
                    entry["review"] = excerpt
                selected.append(entry)
    if not any(item["path"] == pin["intendedQaicPath"] for item in selected):
        raise ValueError("the intended QAIC executable is absent from the checked archive")
    if not any(item["path"] == pin["requiredRecipePath"] for item in selected):
        raise ValueError("the required build_idl recipe is absent from the checked archive")
    return {
        "schemaVersion": 1,
        "kind": "hexagon-SDK-data-only-inventory",
        "archive": pin["asset"],
        "archiveEntriesRead": entries,
        "declaredUncompressedBytes": declared,
        "linkEntriesNotExtracted": links,
        "selectedBytesRead": selected_bytes,
        "selectedFiles": sorted(selected, key=lambda item: item["path"]),
        "sdkContentsExtracted": False,
        "sdkCodeExecuted": False,
        "generatorExecuted": False,
        "generatedSourceUploaded": False,
        "licenseReview": "pending; bounded necessary review excerpts and file identities only",
        "generatorInvocationReviewed": False,
        "deviceCodeLoaded": False,
        "profileEligible": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--pin", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    pin = json.loads(args.pin.read_text(encoding="utf-8-sig"))
    if pin["schemaVersion"] != 1 or pin["generatorExecutionAllowed"] or pin["deviceExecutionAllowed"]:
        raise ValueError("the SDK inventory pin does not authorize this operation")
    result = inventory(args.archive, pin)
    result["inventorySourceSha256"] = sha256(Path(__file__).read_bytes())
    result["inventoryPinSha256"] = sha256(args.pin.read_bytes())
    output = (json.dumps(result, ensure_ascii=True, indent=2) + "\n").encode("ascii")
    if len(output) > pin["limits"]["receiptBytes"]:
        raise ValueError("SDK inventory receipt exceeds its byte bound")
    with args.output.open("xb") as target:
        target.write(output)
        target.flush()
    print(json.dumps({"inventoryWritten": True, "selectedFiles": len(result["selectedFiles"]), "sdkCodeExecuted": False}))


if __name__ == "__main__":
    main()
