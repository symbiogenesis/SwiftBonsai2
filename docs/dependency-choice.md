# Broker dependency evidence

Comparisons target native Windows 11 ARM64 with the pinned nightly, new trait
solver and Polonius. Correctness and unchanged strict request policy precede
timing. Compiler acceptance is not evidence of faster inference.

## Rejected: SIMD-JSON 0.18.1

The focused `candidates_reject_the_same_malformed_schema_and_encodings` regression
found that its Serde adapter accepted the synthetic request
`{"messages":[{"role":"user","content":"\ud800"}]}` and canonicalized its content
to `\u0000`. The production Serde JSON parser rejects the lone high surrogate
with the value-free `ChatSchema` category. This violates the current input
contract and excludes the candidate regardless of throughput. The rejected
adapter, dependency and candidate-only regression were removed after recording
this result. The original experiment is preserved in commit `31d8b2c`; the
focused failure evidence is retained in this session's external diagnostic lane.
