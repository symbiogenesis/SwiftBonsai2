"""Bounded SDK/public declaration and notice metadata; no preprocessing or ABI verdict."""

import codecs
import hashlib
import json
import re


def token_hash(tokens):
    return hashlib.sha256(json.dumps(tokens, ensure_ascii=True, separators=(",", ":")).encode("ascii")).hexdigest()


def decoded_text(data):
    """Decode known BOMs, then strict UTF-8 and labeled uncertain Windows fallbacks."""
    attempts = []
    for bom, encoding in ((codecs.BOM_UTF32_LE, "utf-32"), (codecs.BOM_UTF32_BE, "utf-32"),
                          (codecs.BOM_UTF16_LE, "utf-16"), (codecs.BOM_UTF16_BE, "utf-16"),
                          (codecs.BOM_UTF8, "utf-8-sig")):
        if data.startswith(bom):
            try:
                return data.decode(encoding), {"encoding": encoding, "basis": "explicit-BOM", "uncertain": False}
            except UnicodeError:
                return None, {"encoding": encoding, "basis": "invalid-BOM-declared-text", "uncertain": True}
    try:
        text = data.decode("utf-8")
        if "\0" not in text:
            return text, {"encoding": "utf-8", "basis": "strict-decode-no-BOM", "uncertain": False}
        attempts.append("UTF8-decoded-with-NULs")
    except UnicodeError:
        attempts.append("strict-UTF8-failed")
    # These are hypotheses about text encoding, not an identification of terms.
    if len(data) >= 4 and len(data) % 2 == 0:
        for parity, encoding in ((1, "utf-16-le"), (0, "utf-16-be")):
            if data[parity::2].count(0) / len(data[parity::2]) > 0.6:
                try:
                    return data.decode(encoding), {"encoding": encoding, "basis": "NUL-pattern-Windows-text-fallback-no-BOM", "uncertain": True, "priorAttempts": attempts}
                except UnicodeError:
                    attempts.append(encoding + "-failed")
    for encoding in ("cp1252", "latin-1"):
        try:
            text = data.decode(encoding)
        except UnicodeError:
            attempts.append(encoding + "-failed")
            continue
        return text, {"encoding": encoding, "basis": "Windows-1252-fallback-no-BOM" if encoding == "cp1252" else "byte-preserving-Latin1-fallback-no-BOM", "uncertain": True, "priorAttempts": attempts}
    return None, {"encoding": None, "basis": "text-encoding-unresolved", "uncertain": True, "priorAttempts": attempts}


def legal_review(data, words):
    text, encoding = decoded_text(data)
    result = {"encodingObservation": encoding, "sdkToolTermsQualified": False,
              "generatedOutputRightsQualified": False, "keywordMatchesEstablishGrant": False,
              "absenceOfTermsEstablished": False}
    if text is None:
        return {**result, "status": "pending-text-decoding"}
    controls = sum(ord(char) < 32 and char not in "\r\n\t\f" for char in text)
    result["nonTextControlCharacters"] = controls
    if controls > max(2, len(text) // 100):
        return {**result, "status": "pending-text-plausibility; no-binary-classification-inferred"}
    terms = {"qaic-name": r"\bqaic\b", "IDL-compiler-phrase": r"\b(?:another\s+)?idl\s+compiler\b",
             "SDK-phrase": r"\b(?:hexagon\s+)?sdk\b", "license-agreement-phrase": r"licen[sc]e\s+agreement",
             "product-license-phrase": r"product\s+licen[sc]e", "redistribution-word": r"\bredistribution\b",
             "patent-word": r"\bpatent\b", "confidential-word": r"\bconfidential\b",
             "proprietary-word": r"\bproprietary\b", "GHC-Haskell-name": r"\b(?:ghc|haskell)\b"}
    matches = {}
    for name, pattern in terms.items():
        found = list(re.finditer(pattern, text, flags=re.IGNORECASE))
        matches[name] = {"count": len(found), "firstLines": [text.count("\n", 0, item.start()) + 1 for item in found[:8]]}
    result["termOccurrences"] = matches
    focus = list(re.finditer(terms["qaic-name"], text, flags=re.IGNORECASE))
    if not focus:
        focus = list(re.finditer(terms["license-agreement-phrase"] + "|" + terms["product-license-phrase"], text, flags=re.IGNORECASE))
    if focus:
        match = focus[0]
        before = text[max(0, match.start() - 256):match.start()].split()[-8:]
        after = text[match.start():match.end() + 512].split()
        excerpt = " ".join((before + after)[:words])
        result["necessaryReviewExcerpt"] = {"line": text.count("\n", 0, match.start()) + 1, "wordsMaximum": words, "text": excerpt}
    else:
        result["necessaryReviewExcerpt"] = {"line": 1, "wordsMaximum": words, "text": " ".join(text.split()[:words])}
    result["status"] = "decoded-data-only-review; applicability-and-grant-unresolved"
    return result


def functional_tokens(text, limit):
    """Lex C/IDL tokens, removing comments without touching quoted literals.

    Backslash/newline splices are removed first. Preprocessor directive boundaries
    remain explicit tokens; directives/macros/conditional branches are not evaluated.
    """
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    splices = len(re.findall(r"\\\n", text))
    text = re.sub(r"\\\n", "", text)
    tokens = []
    lines = []
    position = 0
    line = 1
    line_start = True
    preprocessor = False
    directives = []
    current_directive = []
    directive_line = None
    previous_token_end = None

    def emit(value):
        if len(tokens) >= limit or len(value) > 4096:
            raise ValueError("functional-token-count-or-length-bound")
        tokens.append(value)
        lines.append(line)
        if preprocessor and value not in {"<PP-BEGIN>", "<PP-END>"}:
            current_directive.append(value)

    def end_directive():
        nonlocal preprocessor, current_directive, directive_line
        if preprocessor:
            emit("<PP-END>")
            directives.append({"line": directive_line, "tokens": current_directive})
            current_directive = []
            directive_line = None
            preprocessor = False

    while position < len(text):
        char = text[position]
        if char.isspace():
            if char == "\n":
                end_directive()
                line += 1
                line_start = True
            position += 1
            continue
        if text.startswith("//", position):
            end = text.find("\n", position + 2)
            position = len(text) if end < 0 else end
            continue
        if text.startswith("/*", position):
            end = text.find("*/", position + 2)
            if end < 0:
                raise ValueError("unterminated-block-comment")
            comment = text[position:end + 2]
            if "\n" in comment:
                end_directive()
                line += comment.count("\n")
                line_start = True
            position = end + 2
            continue
        if char == "#" and line_start:
            preprocessor = True
            directive_line = line
            emit("<PP-BEGIN>")
        if preprocessor and current_directive[:2] == ["#", "include"] and len(current_directive) == 2 and char == "<":
            end = text.find(">", position + 1)
            newline = text.find("\n", position + 1)
            if end < 0 or (newline >= 0 and newline < end):
                raise ValueError("unterminated-include-header-name")
            emit(text[position:end + 1])
            position = end + 1
            previous_token_end = position
            line_start = False
            continue
        if preprocessor and current_directive[:2] == ["#", "define"] and len(current_directive) == 3 and char == "(":
            emit("<PP-MACRO-FUNCTIONLIKE>" if previous_token_end == position else "<PP-MACRO-OBJECTLIKE-PAREN>")
        literal = re.match(r"(?:u8|u|U|L)?[\"']", text[position:])
        if literal:
            start = position
            quote = literal[0][-1]
            position += len(literal[0])
            while position < len(text):
                if text[position] == "\\":
                    position += 2
                elif text[position] == quote:
                    position += 1
                    break
                elif text[position] == "\n":
                    raise ValueError("newline-in-unclosed-literal")
                else:
                    position += 1
            else:
                raise ValueError("unterminated-quoted-literal")
            emit(text[start:position])
        else:
            match = re.match(r"[A-Za-z_][A-Za-z0-9_]*|(?:\.?[0-9])(?:[A-Za-z0-9_.]|(?<=[eEpP])[+-])*|>>=|<<=|\.\.\.|->|\+\+|--|==|!=|<=|>=|&&|\|\||\+=|-=|\*=|/=|%=|&=|\|=|\^=|<<|>>|::|##|[{}\[\]();,:?~!+\-*/%&|^<>=.#]", text[position:])
            if match is None:
                raise ValueError("unsupported-functional-source-character")
            emit(match[0])
            position += len(match[0])
        previous_token_end = position
        line_start = False
    end_directive()
    return tokens, lines, directives, splices


def declaration_candidates(tokens, lines, limits):
    """Retain lexical candidates only; this deliberately is not a C type checker."""
    code = []
    code_lines = []
    preprocessor = False
    for token, line in zip(tokens, lines):
        if token == "<PP-BEGIN>":
            preprocessor = True
        elif token == "<PP-END>":
            preprocessor = False
        elif not preprocessor:
            code.append(token)
            code_lines.append(line)
    declarations = []
    current = []
    start_line = None
    braces = parentheses = brackets = 0
    position = 0
    while position < len(code):
        token = code[position]
        if not current and code[position:position + 3] == ["extern", '"C"', "{"]:
            position += 3
            continue
        if not current and token == "}":
            position += 1
            continue
        if start_line is None:
            start_line = code_lines[position]
        current.append(token)
        parentheses += (token == "(") - (token == ")")
        brackets += (token == "[") - (token == "]")
        braces += (token == "{") - (token == "}")
        if min(braces, parentheses, brackets) < 0:
            return {"status": "pending-declaration-review", "reason": "unbalanced-declaration-delimiter",
                    "declarations": declarations, "unprocessedTailTokens": len(code) - position,
                    "typeCheckingExecuted": False}
        if token == ";" and braces == parentheses == brackets == 0:
            names = sorted(set(value for value in current if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value) and re.search(r"dspqueue|fastrpc|remote|callback|rpcmem", value, flags=re.IGNORECASE)))
            if names or any(value in current for value in ("typedef", "enum")):
                if len(current) > limits["declarationTokens"] or len(declarations) >= limits["functionalDeclarations"]:
                    return {"status": "pending-declaration-review", "reason": "functional-declaration-output-bound",
                            "declarations": declarations, "unprocessedTailTokens": len(code) - position,
                            "typeCheckingExecuted": False}
                callbacks = [current[index + 2] for index in range(len(current) - 3) if current[index:index + 2] == ["(", "*"] and current[index + 3] == ")"]
                methods = sorted(set(current[index] for index in range(len(current) - 1)
                                     if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", current[index]) and current[index + 1] == "("
                                     and current[index] not in {"if", "while", "for", "switch", "sizeof", "defined", "_Alignof", "__attribute__", "__declspec"}))
                alias = None
                if "typedef" in current:
                    alias = callbacks[-1] if callbacks else next((value for value in reversed(current[:-1])
                                                                 if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value)), None)
                declarations.append({"startLine": start_line, "identifierCandidates": names,
                                     "typedefPresent": "typedef" in current, "enumPresent": "enum" in current,
                                     "typedefAliasNameCandidate": alias,
                                     "methodNameCandidates": methods,
                                     "callbackTypedefNameCandidates": callbacks if "typedef" in current else [],
                                     "normalizedTokenSha256": token_hash(current), "tokens": current,
                                     "conditionalPreprocessorNotEvaluated": True})
            current = []
            start_line = None
        position += 1
    return {"status": "lexical-candidates-only-not-an-ABI-parser", "declarations": declarations,
            "unclosedOrNonDeclarationTailTokens": len(current), "typeCheckingExecuted": False}


def functional_metadata(data, limits):
    text, encoding = decoded_text(data)
    result = {"encodingObservation": encoding, "macrosEvaluated": False, "ABIEquivalenceQualified": False,
              "normalization": "C-phase2-line-splices; comment-aware-C-IDL-lexing; canonical-JSON-ASCII-token-array; explicit-preprocessor-boundaries-and-macro-adjacency; literal-include-header-names"}
    if text is None or "\0" in text:
        return {**result, "status": "pending-functional-text-decoding"}
    try:
        tokens, lines, directives, splices = functional_tokens(text, limits["functionalTokens"])
    except ValueError as error:
        return {**result, "status": "pending-functional-token-review", "reason": str(error)}
    result.update({"status": "normalized-functional-tokens-observed-only", "normalizedTokenSha256": token_hash(tokens),
                   "tokenCount": len(tokens), "lineSplicesRemoved": splices})
    if len(directives) > limits["preprocessorDirectives"]:
        result["preprocessorStatus"] = "pending-preprocessor-metadata-bound"
    else:
        result["preprocessorDirectives"] = directives
        result["includeDirectives"] = [entry for entry in directives if entry["tokens"][:2] == ["#", "include"]]
        result["macroDefinitions"] = [entry for entry in directives if entry["tokens"][:2] == ["#", "define"]]
    try:
        result["declarationCandidates"] = declaration_candidates(tokens, lines, limits)
    except ValueError as error:
        result["declarationCandidates"] = {"status": "pending-declaration-review", "reason": str(error)}
    return result


def compare_functional(sdk, public, limits):
    left = functional_metadata(sdk, limits)
    right = functional_metadata(public, limits)
    complete = "normalizedTokenSha256" in left and "normalizedTokenSha256" in right
    indexes = []
    for metadata in (left, right):
        index = {}
        for declaration in metadata.get("declarationCandidates", {}).get("declarations", []):
            names = declaration["identifierCandidates"] + declaration.get("methodNameCandidates", []) + declaration["callbackTypedefNameCandidates"]
            if declaration.get("typedefAliasNameCandidate"):
                names.append(declaration["typedefAliasNameCandidate"])
            for name in names:
                index.setdefault(name, set()).add(declaration["normalizedTokenSha256"])
        indexes.append(index)
    shared = sorted(indexes[0].keys() & indexes[1].keys())
    named = [{"identifier": name, "sdkCandidateTokenHashes": sorted(indexes[0][name]),
              "publicCandidateTokenHashes": sorted(indexes[1][name]),
              "fullLexicalCandidateSetsEqual": indexes[0][name] == indexes[1][name]} for name in shared[:limits["functionalDeclarations"]]]
    return {"status": "lexical-comparison-only" if complete else "pending-functional-token-review",
            "sdk": left, "public": right, "rawBytesEqual": sdk == public,
            "normalizedTokenStreamsEqual": left["normalizedTokenSha256"] == right["normalizedTokenSha256"] if complete else None,
            "sameIdentifierCandidateComparisons": named, "sameIdentifierCandidateComparisonsTruncated": len(named) != len(shared),
            "matchingNamesOrTokensProveABIEquivalence": False, "WindowsRuntimeCompatibilityQualified": False,
            "callbackLifetimeQualified": False, "cacheAndBufferOwnershipQualified": False,
            "installedDriverAbiQualified": False, "deviceInvocationAuthorized": False}
