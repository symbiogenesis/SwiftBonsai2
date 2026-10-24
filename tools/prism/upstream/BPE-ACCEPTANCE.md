# BPE roundtrip acceptance inputs, version 2

The complete safe Rust port of `tests/test-tokenizer-1-bpe.cpp` accepts a
vocabulary and the original optional `--ignore-merges` flag. The eight example
invocations in the pinned CMake file are commented out. This version explicitly
selects compatible inputs without changing the source's assertions.

Default source acceptance runs `aquila`, `gpt-2`, `gpt-neox`, `mpt`, `refact`, and
`starcoder` with the original false flag, plus `qwen2` with the true flag. The
unchanged bundled Qwen2 asset is 5,928,681 bytes, SHA-256
`44c2f46b715f585c6ab513970e8a006bfa5badd6108560054921cf598d154d8c`.
It has 151,936 token entries, all 256 encoded base bytes, and metadata
`tokenizer.ggml.model=gpt2`, `tokenizer.ggml.pre=qwen2`. The pinned production
Qwen2 branch disables whitespace cleanup. These are input-selection facts,
not a passing full-source verdict; hosted CI must qualify all token rows and
the complete Unicode domain for the new input.

Every original predicate remains: vocabulary type and loading checks; all
vocabulary tokens; invalid-argument handling; the approved strict invalid-UTF8
piece skip; the `tokens.size() <= 1` check when requested; exact detokenized-byte
equality; both original Unicode exclusions; the U+2581 exemption; worker strides
and shared stop behavior. Successful full runs require 456,703 Unicode rows and
456,702 equality predicates per vocabulary. The true-flag receipt must contain
a positive actual count of single-token predicates. Neither the flag nor this
selection changes production vocabulary metadata or tokenizer behavior.

The original Falcon and Llama-BPE inputs remain selectable diagnostics with
their actual status. Hosted run 40131582460 exposed two input incompatibilities:

- Falcon has no normal encoded-byte U+0104, raw U+0004, or byte fallback entry
  representing byte 04. Its 65,012 normal and 12 control tokens cannot satisfy
  the source's universal Unicode roundtrip assertion for that byte.
- Llama-BPE token 24268 has raw bytes `202124` (` !$`). Production cleanup
  changes this to `2124` (`!$`), which tokenizes as two tokens. The original
  `--ignore-merges` cardinality check therefore fails. GPT-2 is also unsuitable
  for that flag: token 14512 normalizes ` !=` to `!=`, whose direct token is
  absent. Dropping the flag would remove required source coverage.

This is not repaired by inventing a GGUF cleanup key: the pinned loader does not
read one. Meta's [Llama 3 tokenizer](https://github.com/meta-llama/llama-models/blob/main/models/llama3/tokenizer.py)
delegates decoding directly to tiktoken. The official Transformers
[Llama regression](https://github.com/huggingface/transformers/blob/main/tests/models/llama/test_tokenization_llama.py)
also distinguishes shipped cleanup settings from BPE decoding behavior. No
publisher setting is rewritten or production implementation patched here.

The typed diagnostic configuration passed through `--diagnostic` is:

```json
{"Kind":"tokenizer-bpe-roundtrip","Vocabulary":"falcon","IgnoreMerges":false}
```

Use `"Vocabulary":"llama-bpe","IgnoreMerges":true` for the other original
variant. Any named pinned BPE vocabulary and either flag remain selectable.
Diagnostics retain all original checks and actual failures; their distinct
diagnostic IDs, `SourceId`, `SourceCount:0`, and diagnostic measure prevent them
from being counted as a source pass. Input hashes and caller arguments appear
in the receipts. The acceptance manifest preserves all original asset identities
and records the explicit version 2 selection and previous hosted evidence.
