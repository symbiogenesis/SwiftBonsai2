"""Read a pinned SDK archive as data; never extract or execute SDK contents.

The hosted supervisor retains a deny-write/delete handle for the verified archive
through this process's confirmed exit. This reader verifies the same open bytes
again, then streams bounded tar metadata and selected file contents into hashes.
"""

import argparse
import hashlib
import json
import re
import struct
import tarfile
from pathlib import Path, PurePosixPath

from sdk_functional_review import compare_functional, decoded_text, functional_metadata, legal_review


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
    if "/build/cmake/" in lower and lower.endswith(".cmake"):
        return "build-recipe-helper"
    if lower.endswith(".idl") and (
        "/incs/" in lower or "/ipc/fastrpc/" in lower or leaf in {"remote.idl", "aeestddef.idl"}
    ):
        return "sdk-idl-input-candidate"
    if leaf in {"remote.h", "remote_wos_ext.h", "dspqueue.h", "rpcmem.h"}:
        return "sdk-rpc-header-candidate"
    if any(term in leaf for term in ("license", "licence", "eula", "notice", "copyright", "terms", "agreement", "copying")):
        return "license-review"
    return None


def review(data, kind, words):
    if kind not in {"build-idl-recipe", "license-review"}:
        return None
    if kind == "license-review":
        return legal_review(data, words)
    try:
        text = data.decode("utf-8-sig")
    except UnicodeError:
        return {"textEncoding": "not-UTF8", "excerpt": None}
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


def notice_metadata(data):
    """Classify bounded source/binary notice markers without interpreting rights."""
    lower = data.lower()
    markers = {
        "copyright": b"copyright",
        "all-rights-reserved": b"all rights reserved",
        "qualcomm-name": b"qualcomm",
        "confidential-marker": b"confidential",
        "proprietary-marker": b"proprietary",
        "redistribution-word": b"redistribution",
        "distribution-word": b"distribution",
        "license-word": b"license",
        "agreement-word": b"agreement",
        "terms-word": b"terms",
        "bsd-name": b"bsd",
        "apache-name": b"apache",
        "gpl-name": b"general public license",
    }
    if pe_metadata(data)["validPeHeader"]:
        text = None
        encoding = {"encoding": None, "basis": "validated-PE-header; text-decoding-not-attempted", "uncertain": False}
    else:
        text, encoding = decoded_text(data)
    if text is not None:
        lower = text.lower().encode("utf-8")
    result = {
        "markersPresent": [name for name, marker in markers.items() if marker in lower],
        "licenseRightsInterpretationReviewed": False,
        "encodingObservation": encoding,
    }
    if text is not None:
        spdx = re.findall(r"SPDX-License-Identifier:\s*([^\r\n*]{1,128})", text)
        result["spdxIdentifiersMentioned"] = sorted(set(item.strip() for item in spdx))[:16]
        result["copyrightMarkerLines"] = [
            index + 1 for index, line in enumerate(text.splitlines()) if "copyright" in line.lower()
        ][:32]
        result["classificationScope"] = "text-markers-only-not-a-license-grant"
    else:
        result["classificationScope"] = "raw-byte-markers-only-not-a-license-grant; no-binary-classification-from-decode-failure"
    return result


def pe_metadata(data):
    if len(data) < 64 or data[:2] != b"MZ":
        return {"validPeHeader": False}
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset < 64 or offset + 24 > len(data) or data[offset:offset + 4] != b"PE\0\0":
        return {"validPeHeader": False}
    machine = struct.unpack_from("<H", data, offset + 4)[0]
    return {"validPeHeader": True, "machine": machine, "machineHex": f"{machine:04x}"}


def cmake_statements(text):
    """Tokenize balanced CMake commands as data; perform no expansion/evaluation."""
    statements = []
    cursor = 0
    while cursor < len(text):
        if text[cursor].isspace():
            cursor += 1
            continue
        if text[cursor] == "#":
            bracket = re.match(r"#\[(=*)\[", text[cursor:])
            if bracket:
                end = text.find("]" + bracket[1] + "]", cursor + len(bracket[0]))
                if end < 0:
                    raise ValueError("unterminated CMake bracket comment")
                cursor = end + len(bracket[1]) + 2
            else:
                end = text.find("\n", cursor)
                cursor = len(text) if end < 0 else end + 1
            continue
        head = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(", text[cursor:])
        if head is None:
            raise ValueError("unsupported CMake statement boundary")
        start = cursor
        cursor += len(head[0])
        depth = 1
        tokens = []
        token = []
        quoted = False
        while cursor < len(text) and depth:
            char = text[cursor]
            if char == "\\" and cursor + 1 < len(text):
                token.extend(text[cursor:cursor + 2])
                cursor += 2
                continue
            if char == '"':
                quoted = not quoted
                token.append(char)
                cursor += 1
                continue
            if not quoted and char == "#":
                if token:
                    tokens.append("".join(token))
                    token = []
                bracket = re.match(r"#\[(=*)\[", text[cursor:])
                if bracket:
                    end = text.find("]" + bracket[1] + "]", cursor + len(bracket[0]))
                    if end < 0:
                        raise ValueError("unterminated CMake argument comment")
                    cursor = end + len(bracket[1]) + 2
                else:
                    end = text.find("\n", cursor)
                    cursor = len(text) if end < 0 else end + 1
                continue
            if not quoted and char == "[":
                bracket = re.match(r"\[(=*)\[", text[cursor:])
                if bracket:
                    end = text.find("]" + bracket[1] + "]", cursor + len(bracket[0]))
                    if end < 0:
                        raise ValueError("unterminated CMake bracket argument")
                    extent = end + len(bracket[1]) + 2
                    token.append(text[cursor:extent])
                    cursor = extent
                    continue
            if not quoted and char in "()":
                if token:
                    tokens.append("".join(token))
                    token = []
                depth += 1 if char == "(" else -1
                if depth:
                    tokens.append(char)
            elif not quoted and char.isspace():
                if token:
                    tokens.append("".join(token))
                    token = []
            else:
                token.append(char)
            cursor += 1
        if depth or quoted:
            raise ValueError("unterminated CMake command")
        statements.append({
            "command": head[1].lower(),
            "tokens": tokens,
            "startLine": text.count("\n", 0, start) + 1,
            "endLine": text.count("\n", 0, cursor) + 1,
        })
    return statements


def recipe_metadata(recipes, pin):
    limits = pin["limits"]
    definitions = {}
    outside = []
    parse_failures = []
    for path, data in recipes.items():
        try:
            statements = cmake_statements(data.decode("utf-8-sig"))
        except (UnicodeError, ValueError):
            parse_failures.append(path)
            continue
        cursor = 0
        while cursor < len(statements):
            statement = statements[cursor]
            if statement["command"] not in {"function", "macro"}:
                outside.append((path, statement))
                cursor += 1
                continue
            end = cursor + 1
            depth = 1
            while end < len(statements) and depth:
                command = statements[end]["command"]
                if command in {"function", "macro"}:
                    depth += 1
                elif command in {"endfunction", "endmacro"}:
                    depth -= 1
                end += 1
            if depth or not statement["tokens"]:
                parse_failures.append(path)
                break
            name = statement["tokens"][0].lower()
            definitions.setdefault(name, []).append((path, statements[cursor:end]))
            cursor = end
    pending = ["build_idl"]
    chosen = []
    seen = set()
    variables = {"proj-qaic_src", "HEXAGON_SDK_ROOT", "cmdLineOptions"}
    while pending:
        name = pending.pop(0)
        if name in seen:
            continue
        seen.add(name)
        matches = definitions.get(name, [])
        if len(matches) != 1 or len(chosen) >= limits["recipeFunctions"]:
            raise ValueError("required CMake function is absent, ambiguous or exceeds its bound")
        path, statements = matches[0]
        emitted = []
        for statement in statements:
            item = dict(statement)
            if item["command"] == "message":
                item["tokens"] = []
                item["nonfunctionalMessageTextOmitted"] = True
            else:
                item["tokens"] = list(item["tokens"])
                for index, token in enumerate(item["tokens"][:-1]):
                    if token.upper() == "COMMENT":
                        item["tokens"][index + 1] = "<nonfunctional-comment-text-omitted>"
            emitted.append(item)
        chosen.append({"name": name, "sourcePath": path, "statements": emitted})
        for statement in statements:
            if statement["command"] in definitions and statement["command"] not in seen:
                pending.append(statement["command"])
            for token in statement["tokens"]:
                variables.update(re.findall(r"\$(?:ENV)?\{([^}]+)\}", token))
    setup = []
    for path, statement in outside:
        if statement["command"] not in {"set", "list", "include", "get_filename_component"}:
            continue
        tokens = statement["tokens"]
        if any(token in variables or re.search(r"qaic|include|/incs|cmdLineOptions", token, re.IGNORECASE) for token in tokens):
            setup.append({"sourcePath": path, **statement})
    all_statements = [statement for function in chosen for statement in function["statements"]] + setup
    if len(all_statements) > limits["recipeStatements"]:
        raise ValueError("selected functional CMake statement metadata exceeds its bound")
    for statement in all_statements:
        if len(statement["tokens"]) > limits["recipeStatementTokens"] or any(len(token) > limits["recipeTokenCharacters"] for token in statement["tokens"]):
            raise ValueError("selected CMake argument token metadata exceeds its bound")
    return {
        "status": "functional-token-metadata-collected-unreviewed",
        "scope": "functional-command-token-metadata-only-no-comments-or-CMake-evaluation",
        "functions": chosen,
        "relatedTopLevelSetup": setup,
        "otherRecipeFilesNotParsed": parse_failures,
        "variableExpansionExecuted": False,
        "generatorCommandAuthorized": False,
    }


def inventory(archive, pin, public_input_directory):
    limits = pin["limits"]
    selected = []
    recipes = {}
    comparison_bytes = {}
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
                if name == pin["requiredRecipePath"] and (
                    member.size != pin["requiredRecipeBytes"] or entry["sha256"] != pin["requiredRecipeSha256"]
                ):
                    raise ValueError("the observed CMake recipe differs from its newly reviewed identity")
                entry["noticeClassification"] = notice_metadata(data)
                if kind in {"sdk-idl-input-candidate", "sdk-rpc-header-candidate"}:
                    entry["functionalMetadata"] = functional_metadata(data, limits)
                for comparison in pin["functionalComparisons"]:
                    if name == comparison["sdkPath"]:
                        if member.size != comparison["sdkBytes"] or entry["sha256"] != comparison["sdkSha256"]:
                            raise ValueError("SDK functional-comparison input differs from its independently observed identity")
                        comparison_bytes[name] = data
                if name == pin["requiredNotice"]["path"] and (
                    member.size != pin["requiredNotice"]["bytes"] or entry["sha256"] != pin["requiredNotice"]["sha256"]
                ):
                    raise ValueError("SDK NOTICE differs from its independently observed identity")
                if kind == "qaic-generator":
                    entry["peHeader"] = pe_metadata(data)
                if kind in {"build-idl-recipe", "build-recipe-helper"}:
                    recipes[name] = data
                excerpt = review(data, kind, limits["reviewWordsPerFile"])
                if excerpt is not None:
                    entry["review"] = excerpt
                selected.append(entry)
    if not any(item["path"] == pin["intendedQaicPath"] for item in selected):
        raise ValueError("the intended QAIC executable is absent from the checked archive")
    if not any(item["path"] == pin["requiredRecipePath"] for item in selected):
        raise ValueError("the required build_idl recipe is absent from the checked archive")
    if not any(item["path"] == pin["requiredNotice"]["path"] for item in selected):
        raise ValueError("the independently observed SDK NOTICE is absent")
    try:
        recipe = recipe_metadata(recipes, pin)
    except ValueError as error:
        # An unsupported recipe never becomes guessed flags or permissions.
        # Preserve the independent bounded file/license observations for review.
        recipe = {
            "status": "pending-functional-token-review",
            "reason": str(error),
            "variableExpansionExecuted": False,
            "generatorCommandAuthorized": False,
        }
    comparisons = []
    for comparison in pin["functionalComparisons"]:
        sdk = comparison_bytes.get(comparison["sdkPath"])
        public_path = public_input_directory / comparison["publicName"]
        if sdk is None or not public_path.is_file() or public_path.is_symlink():
            raise ValueError("SDK/public declaration comparison omitted a fixed input")
        public = public_path.read_bytes()
        if len(public) != comparison["publicBytes"] or sha256(public) != comparison["publicSha256"]:
            raise ValueError("public declaration comparison input differs from its source pin")
        comparisons.append({"sdkPath": comparison["sdkPath"], "publicName": comparison["publicName"],
                            "sdkRawSha256": sha256(sdk), "publicRawSha256": sha256(public),
                            "comparison": compare_functional(sdk, public, limits)})
    return {
        "schemaVersion": 1,
        "kind": "hexagon-SDK-data-only-inventory",
        "archive": pin["asset"],
        "archiveEntriesRead": entries,
        "declaredUncompressedBytes": declared,
        "linkEntriesNotExtracted": links,
        "selectedBytesRead": selected_bytes,
        "selectedFiles": sorted(selected, key=lambda item: item["path"]),
        "buildIdlRecipe": recipe,
        "sdkPublicFunctionalComparisons": comparisons,
        "functionalComparisonScope": "unpreprocessed-lexical-token-and-declaration-candidates-only; no-ABI-layout-or-runtime-equivalence",
        "WindowsExtensionDeclarationsQualified": False,
        "licenseFilenameSearch": {
            "scope": "all-regular-archive-entry-leaf-names-without-depth-or-prefix-restrictions",
            "substringIndicators": ["license", "licence", "eula", "notice", "copyright", "terms", "agreement", "copying"],
            "matchingNamesEstablishLicenseAbsenceOrRights": False,
        },
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
    parser.add_argument("--public-input-directory", required=True, type=Path)
    args = parser.parse_args()
    pin = json.loads(args.pin.read_text(encoding="utf-8-sig"))
    if pin["schemaVersion"] != 1 or pin["generatorExecutionAllowed"] or pin["deviceExecutionAllowed"]:
        raise ValueError("the SDK inventory pin does not authorize this operation")
    result = inventory(args.archive, pin, args.public_input_directory)
    result["inventorySourceSha256"] = sha256(Path(__file__).read_bytes())
    result["inventoryPinSha256"] = sha256(args.pin.read_bytes())
    result["functionalReviewSourceSha256"] = sha256(Path(__file__).with_name("sdk_functional_review.py").read_bytes())
    output = (json.dumps(result, ensure_ascii=True, indent=2) + "\n").encode("ascii")
    if len(output) > pin["limits"]["receiptBytes"]:
        raise ValueError("SDK inventory receipt exceeds its byte bound")
    with args.output.open("xb") as target:
        target.write(output)
        target.flush()
    print(json.dumps({"inventoryWritten": True, "selectedFiles": len(result["selectedFiles"]), "sdkCodeExecuted": False}))


if __name__ == "__main__":
    main()
