# Prism storage overlay ownership

The immutable LoRA B-storage sharing implementation, native regression and
engine builder are owned by Prism Inference. See its
[overlay guide](https://github.com/symbiogenesis/prism-inference/blob/main/docs/prism-storage-overlay.md)
and [builder](https://github.com/symbiogenesis/prism-inference/blob/main/scripts/Build-PrismEngine.ps1).

Runner consumes reviewed pinned artifacts. The assessment runtime does not
automatically include a source overlay; adapter correctness, throughput and
quality retain their separate [qualification](swift-lora.md).
