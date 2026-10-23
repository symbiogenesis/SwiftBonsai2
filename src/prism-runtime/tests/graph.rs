//! Focused scalar graph, state ownership, and model contract regressions.

use std::collections::BTreeMap;
use std::sync::Arc;

use bonsai_prism_runtime::graph::Qwen35;
use bonsai_prism_runtime::kernels::round_f16;
use bonsai_prism_runtime::model::{Model, Tensor};
use bonsai_prism_runtime::{Decoder, Result};
use serde_json::{Value, json};

fn weight(name: &str, dimensions: &[usize], values: &[f32]) -> Tensor {
    Tensor {
        name: name.to_owned(),
        dimensions: dimensions.to_vec(),
        kind: 0,
        data: Arc::from(
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        ),
    }
}

fn fixture(recurrent: bool) -> Model {
    let metadata = json!({
        "general.architecture": "qwen35",
        "qwen35.block_count": 1,
        "qwen35.context_length": 16,
        "qwen35.embedding_length": 2,
        "qwen35.feed_forward_length": 2,
        "qwen35.attention.head_count": 1,
        "qwen35.attention.head_count_kv": 1,
        "qwen35.attention.key_length": 2,
        "qwen35.attention.value_length": 2,
        "qwen35.attention.layer_norm_rms_epsilon": 0.000_001,
        "qwen35.rope.dimension_count": 2,
        "qwen35.rope.dimension_sections": [1,0,0,0],
        "qwen35.rope.freq_base": 10000.0,
        "qwen35.ssm.state_size": 2,
        "qwen35.ssm.group_count": 1,
        "qwen35.ssm.time_step_rank": 1,
        "qwen35.ssm.inner_size": 2,
        "qwen35.ssm.conv_kernel": 2,
        "qwen35.full_attention_interval": 1,
        "qwen35.attention.recurrent_layers": [recurrent]
    });
    let mut tensors = BTreeMap::new();
    let mut insert = |name: &str, dimensions: &[usize], values: &[f32]| {
        tensors.insert(name.to_owned(), weight(name, dimensions, values));
    };
    insert("token_embd.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
    insert("output.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
    insert("output_norm.weight", &[2], &[1.0, 1.0]);
    insert("blk.0.attn_norm.weight", &[2], &[1.0, 1.0]);
    insert("blk.0.post_attention_norm.weight", &[2], &[1.0, 1.0]);
    insert("blk.0.ffn_up.weight", &[2, 2], &[0.0; 4]);
    insert("blk.0.ffn_gate.weight", &[2, 2], &[0.0; 4]);
    insert("blk.0.ffn_down.weight", &[2, 2], &[0.0; 4]);
    if recurrent {
        insert(
            "blk.0.attn_qkv.weight",
            &[2, 6],
            &[1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        );
        insert("blk.0.attn_gate.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        insert(
            "blk.0.ssm_conv1d.weight",
            &[2, 6],
            &[
                0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0, 0.25, 1.0,
            ],
        );
        insert("blk.0.ssm_dt.bias", &[1], &[0.0]);
        insert("blk.0.ssm_a", &[1], &[-1.0]);
        insert("blk.0.ssm_beta.weight", &[2, 1], &[0.0, 0.0]);
        insert("blk.0.ssm_alpha.weight", &[2, 1], &[0.0, 0.0]);
        insert("blk.0.ssm_norm.weight", &[2], &[1.0, 1.0]);
        insert("blk.0.ssm_out.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
    } else {
        insert("blk.0.attn_q.weight", &[2, 4], &[0.0; 8]);
        insert("blk.0.attn_k.weight", &[2, 2], &[0.0; 4]);
        insert("blk.0.attn_v.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        insert("blk.0.attn_output.weight", &[2, 2], &[1.0, 0.0, 0.0, 1.0]);
        insert("blk.0.attn_q_norm.weight", &[2], &[1.0, 1.0]);
        insert("blk.0.attn_k_norm.weight", &[2], &[1.0, 1.0]);
    }
    Model {
        metadata: metadata.as_object().cloned().unwrap_or_default(),
        tensors,
        identity: "graph-fixture".to_owned(),
    }
}

#[test]
#[allow(
    clippy::suboptimal_flops,
    clippy::manual_midpoint,
    reason = "The hand calculation preserves scalar reference operation ordering on bounded fixture values."
)]
fn full_attention_uses_causal_binary16_values_and_gate() -> Result<()> {
    let mut decoder = Qwen35::new(fixture(false), 16)?;
    decoder.evaluate(0)?;
    let logits = decoder.evaluate(1)?;
    // Zero Q/K gives uniform causal attention. Two orthogonal normalized
    // embeddings, cached in f16, contribute half their average through the gate.
    let cached = round_f16(1.0_f32 / (0.5_f32 + 0.000_001).sqrt());
    let hidden = [cached * 0.25, 1.0 + cached * 0.25];
    let scale = 1.0 / ((hidden[0] * hidden[0] + hidden[1] * hidden[1]) / 2.0 + 0.000_001).sqrt();
    assert!((logits[0] - hidden[0] * scale).abs() < 0.000_001);
    assert!((logits[1] - hidden[1] * scale).abs() < 0.000_001);
    Ok(())
}

#[test]
fn hybrid_state_checkpoint_restores_exact_recurrent_prefix() -> Result<()> {
    let mut decoder = Qwen35::new(fixture(true), 16)?;
    decoder.evaluate(0)?;
    let checkpoint = decoder.checkpoint()?;
    let checkpoint_bytes = decoder.checkpoint_bytes();
    let expected = decoder.evaluate(1)?;
    decoder.evaluate(0)?;
    decoder.restore(&checkpoint)?;
    assert_eq!(decoder.checkpoint_bytes(), checkpoint_bytes);
    assert_eq!(decoder.evaluate(1)?, expected);
    decoder.reset()?;
    let mut fresh = Qwen35::new(fixture(true), 16)?;
    assert_eq!(decoder.evaluate(1)?, fresh.evaluate(1)?);
    Ok(())
}

#[test]
fn checkpoint_rejects_foreign_cache_contract_and_model() -> Result<()> {
    let mut first = Qwen35::new(fixture(false), 16)?;
    first.evaluate(0)?;
    let checkpoint = first.checkpoint()?;
    let mut other_context = Qwen35::new(fixture(false), 8)?;
    assert!(other_context.restore(&checkpoint).is_err());
    let mut other_instance = Qwen35::new(fixture(false), 16)?;
    assert!(other_instance.restore(&checkpoint).is_err());
    let mut other_model = fixture(false);
    other_model.identity = "different-model".to_owned();
    assert!(Qwen35::new(other_model, 16)?.restore(&checkpoint).is_err());
    Ok(())
}

#[test]
fn invalid_tokens_context_and_weights_fail_without_advancing() -> Result<()> {
    let mut decoder = Qwen35::new(fixture(false), 1)?;
    assert!(decoder.evaluate(2).is_err());
    let expected = Qwen35::new(fixture(false), 1)?.evaluate(0)?;
    assert_eq!(decoder.evaluate(0)?, expected);
    assert!(decoder.evaluate(0).is_err());
    let mut bad_shape = fixture(false);
    bad_shape.tensors.insert(
        "blk.0.attn_q.weight".to_owned(),
        weight("blk.0.attn_q.weight", &[2, 2], &[0.0; 4]),
    );
    assert!(Qwen35::new(bad_shape, 1).is_err());
    let mut bad_value = fixture(true);
    bad_value.tensors.insert(
        "blk.0.ssm_a".to_owned(),
        weight("blk.0.ssm_a", &[1], &[f32::NAN]),
    );
    assert!(Qwen35::new(bad_value, 1).is_err());
    Ok(())
}

#[test]
fn malformed_and_unsupported_metadata_is_rejected() {
    for (key, value) in [
        ("qwen35.full_attention_interval", json!(0)),
        ("qwen35.ssm.group_count", json!(0)),
        ("qwen35.rope.dimension_sections", json!([1, 1, 0, 0])),
        ("qwen35.rope.dimension_sections", json!([u64::MAX, 1, 0, 0])),
        ("qwen35.rope.scaling.type", json!("yarn")),
        ("qwen35.attention.causal", json!(false)),
        ("prism.hadamard.block_size", json!(2)),
    ] {
        let mut model = fixture(false);
        model.metadata.insert(key.to_owned(), value);
        assert!(Qwen35::new(model, 1).is_err(), "accepted {key}");
    }
}

#[test]
fn unsupported_rope_overrides_and_unconsumed_tensors_fail_closed() {
    for (key, value) in [
        ("qwen35.rope.scale_linear", json!(2.0)),
        ("qwen35.rope.scaling.alpha", json!(2.0)),
        ("prism.hadamard.unknown_transform", json!(true)),
    ] {
        let mut model = fixture(false);
        model.metadata.insert(key.to_owned(), value);
        assert!(Qwen35::new(model, 1).is_err(), "accepted {key}");
    }
    let mut model = fixture(false);
    model.tensors.insert(
        "output.bias".to_owned(),
        weight("output.bias", &[2], &[1.0, 2.0]),
    );
    assert!(Qwen35::new(model, 1).is_err());
}

#[test]
fn hadamard_embedding_inverse_and_output_rotation_preserve_basis() -> Result<()> {
    let mut plain = Qwen35::new(fixture(false), 16)?;
    let mut latent = fixture(false);
    let root_half = 1.0_f32 / 2.0_f32.sqrt();
    latent.tensors.insert(
        "token_embd.weight".to_owned(),
        weight(
            "token_embd.weight",
            &[2, 2],
            &[root_half, root_half, root_half, -root_half],
        ),
    );
    latent.tensors.insert(
        "output.weight".to_owned(),
        weight(
            "output.weight",
            &[2, 2],
            &[root_half, root_half, root_half, -root_half],
        ),
    );
    for (key, value) in [
        ("prism.hadamard.version", json!(1)),
        ("prism.hadamard.block_size", json!(2)),
        (
            "prism.hadamard.transform",
            json!("normalized-sylvester-walsh-hadamard"),
        ),
        ("prism.hadamard.axis", json!("input-last-dimension")),
        ("prism.hadamard.sign_mode", json!("identity")),
        ("prism.hadamard.weight_names", json!(["output.weight"])),
        (
            "prism.hadamard.inverse_weight_names",
            json!(["token_embd.weight"]),
        ),
    ] {
        latent.metadata.insert(key.to_owned(), value);
    }
    let mut transformed = Qwen35::new(latent, 16)?;
    for token in [0, 1, 1, 0] {
        let expected = plain.evaluate(token)?;
        let actual = transformed.evaluate(token)?;
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }
    Ok(())
}

#[test]
fn explicit_signs_require_complete_valid_widths() {
    let mut model = fixture(false);
    for (key, value) in [
        ("prism.hadamard.version", json!(1)),
        ("prism.hadamard.block_size", json!(2)),
        (
            "prism.hadamard.transform",
            json!("normalized-sylvester-walsh-hadamard"),
        ),
        ("prism.hadamard.axis", json!("input-last-dimension")),
        ("prism.hadamard.sign_mode", json!("explicit")),
        ("prism.hadamard.weight_names", json!(["output.weight"])),
        ("prism.hadamard.sign_widths", Value::Array(Vec::new())),
        ("prism.hadamard.sign_values", Value::Array(Vec::new())),
    ] {
        model.metadata.insert(key.to_owned(), value);
    }
    assert!(Qwen35::new(model, 1).is_err());
}

#[test]
fn tied_latent_embedding_with_explicit_signs_restores_primal_basis() -> Result<()> {
    let mut plain = Qwen35::new(fixture(false), 16)?;
    let mut latent = fixture(false);
    let half = 1.0_f32 / 2.0_f32.sqrt();
    latent.tensors.remove("output.weight");
    // H * diag([-1,+1]) applied to the two primal embedding rows.
    latent.tensors.insert(
        "token_embd.weight".to_owned(),
        weight("token_embd.weight", &[2, 2], &[-half, -half, half, -half]),
    );
    for (key, value) in [
        ("prism.hadamard.version", json!(2)),
        ("prism.hadamard.tied_output", json!(true)),
        ("prism.hadamard.block_size", json!(2)),
        (
            "prism.hadamard.transform",
            json!("normalized-sylvester-walsh-hadamard"),
        ),
        ("prism.hadamard.axis", json!("input-last-dimension")),
        ("prism.hadamard.sign_mode", json!("explicit")),
        ("prism.hadamard.sign_widths", json!([2])),
        ("prism.hadamard.sign_values", json!([-1, 1])),
        (
            "prism.hadamard.weight_names",
            json!(["blk.0.ffn_up.weight"]),
        ),
        (
            "prism.hadamard.inverse_weight_names",
            json!(["token_embd.weight"]),
        ),
    ] {
        latent.metadata.insert(key.to_owned(), value);
    }
    let mut transformed = Qwen35::new(latent, 16)?;
    for token in [1, 0, 1, 1] {
        let expected = plain.evaluate(token)?;
        let actual = transformed.evaluate(token)?;
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }
    Ok(())
}

#[test]
fn grouped_gdn_value_rotation_matches_unfolded_output_weights() -> Result<()> {
    let mut model = fixture(true);
    model
        .metadata
        .insert("qwen35.ssm.group_count".to_owned(), json!(2));
    model
        .metadata
        .insert("qwen35.ssm.time_step_rank".to_owned(), json!(4));
    model
        .metadata
        .insert("qwen35.ssm.inner_size".to_owned(), json!(8));
    let qkv: Vec<f32> = (0..16)
        .flat_map(|index| {
            if index % 2 == 0 {
                [1.0, 0.25]
            } else {
                [0.25, 1.0]
            }
        })
        .collect();
    model.tensors.insert(
        "blk.0.attn_qkv.weight".to_owned(),
        weight("blk.0.attn_qkv.weight", &[2, 16], &qkv),
    );
    let gate = [
        1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0, 0.5, 0.25, 0.0, 0.0, 0.25, 0.75, 0.0, 0.0, 0.75,
    ];
    model.tensors.insert(
        "blk.0.attn_gate.weight".to_owned(),
        weight("blk.0.attn_gate.weight", &[2, 8], &gate),
    );
    model.tensors.insert(
        "blk.0.ssm_conv1d.weight".to_owned(),
        weight("blk.0.ssm_conv1d.weight", &[2, 16], &[0.25, 1.0].repeat(16)),
    );
    for (name, dims, values) in [
        ("blk.0.ssm_dt.bias", vec![4], vec![0.0; 4]),
        ("blk.0.ssm_a", vec![4], vec![-1.0; 4]),
        ("blk.0.ssm_beta.weight", vec![2, 4], vec![0.0; 8]),
        ("blk.0.ssm_alpha.weight", vec![2, 4], vec![0.0; 8]),
    ] {
        model
            .tensors
            .insert(name.to_owned(), weight(name, &dims, &values));
    }
    let first_row = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    let second_row = [1.0, 0.875, 0.75, 0.625, 0.5, 0.375, 0.25, 0.125];
    let output = [first_row, second_row].concat();
    model.tensors.insert(
        "blk.0.ssm_out.weight".to_owned(),
        weight("blk.0.ssm_out.weight", &[8, 2], &output),
    );
    let mut latent = Model {
        metadata: model.metadata.clone(),
        tensors: model.tensors.clone(),
        identity: model.identity.clone(),
    };
    // P maps [key0/repeat0,key1/repeat0,key0/repeat1,key1/repeat1]
    // to [key0/repeat0,key0/repeat1,key1/repeat0,key1/repeat1].
    let half = 1.0_f32 / 2.0_f32.sqrt();
    let mut folded = Vec::new();
    for original in [first_row, second_row] {
        for head in [0, 2, 1, 3] {
            let first = original[2 * head];
            let second = original[2 * head + 1];
            folded.extend([(first + second) * half, (first - second) * half]);
        }
    }
    latent.tensors.insert(
        "blk.0.ssm_out.weight".to_owned(),
        weight("blk.0.ssm_out.weight", &[8, 2], &folded),
    );
    for (key, value) in [
        ("prism.hadamard.version", json!(1)),
        ("prism.hadamard.block_size", json!(2)),
        (
            "prism.hadamard.transform",
            json!("normalized-sylvester-walsh-hadamard"),
        ),
        ("prism.hadamard.axis", json!("input-last-dimension")),
        ("prism.hadamard.sign_mode", json!("identity")),
        (
            "prism.hadamard.weight_names",
            json!(["blk.0.ssm_out.weight"]),
        ),
        ("prism.hadamard.gdn_v_grouped", json!(true)),
    ] {
        latent.metadata.insert(key.to_owned(), value);
    }
    let mut plain = Qwen35::new(model, 16)?;
    let mut transformed = Qwen35::new(latent, 16)?;
    for token in [0, 1, 0, 1] {
        let expected = plain.evaluate(token)?;
        let actual = transformed.evaluate(token)?;
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 0.000_002);
        }
    }
    Ok(())
}

fn wire_text(output: &mut Vec<u8>, text: &str) -> Result<()> {
    output.extend(u64::try_from(text.len())?.to_le_bytes());
    output.extend(text.as_bytes());
    Ok(())
}

fn wire_value(value: &Value) -> Result<(u32, Vec<u8>)> {
    let mut output = Vec::new();
    let kind = match value {
        Value::Bool(value) => {
            output.push(u8::from(*value));
            7
        }
        Value::String(value) => {
            wire_text(&mut output, value)?;
            8
        }
        Value::Number(value) => {
            if let Some(value) = value.as_u64() {
                output.extend(value.to_le_bytes());
                10
            } else {
                output.extend(value.as_f64().ok_or("fixture number")?.to_le_bytes());
                12
            }
        }
        Value::Array(values) => {
            let first = values.first().ok_or("fixture array must be nonempty")?;
            let (kind, _) = wire_value(first)?;
            output.extend(kind.to_le_bytes());
            output.extend(u64::try_from(values.len())?.to_le_bytes());
            for value in values {
                let (item_kind, bytes) = wire_value(value)?;
                if item_kind != kind {
                    return Err("fixture array types differ".into());
                }
                output.extend(bytes);
            }
            9
        }
        _ => return Err("unsupported fixture value".into()),
    };
    Ok((kind, output))
}

fn wire_model(model: &Model) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    output.extend(0x4655_4747_u32.to_le_bytes());
    output.extend(3_u32.to_le_bytes());
    output.extend(u64::try_from(model.tensors.len())?.to_le_bytes());
    output.extend(u64::try_from(model.metadata.len())?.to_le_bytes());
    for (key, value) in &model.metadata {
        wire_text(&mut output, key)?;
        let (kind, bytes) = wire_value(value)?;
        output.extend(kind.to_le_bytes());
        output.extend(bytes);
    }
    let mut payload = Vec::new();
    for tensor in model.tensors.values() {
        payload.resize(payload.len().div_ceil(32) * 32, 0);
        wire_text(&mut output, &tensor.name)?;
        output.extend(u32::try_from(tensor.dimensions.len())?.to_le_bytes());
        for &dimension in &tensor.dimensions {
            output.extend(u64::try_from(dimension)?.to_le_bytes());
        }
        output.extend(tensor.kind.to_le_bytes());
        output.extend(u64::try_from(payload.len())?.to_le_bytes());
        payload.extend_from_slice(&tensor.data);
    }
    output.resize(output.len().div_ceil(32) * 32, 0);
    output.extend(payload);
    Ok(output)
}

struct DriverArtifact {
    directory: std::path::PathBuf,
    path: std::path::PathBuf,
}

impl Drop for DriverArtifact {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[test]
fn native_driver_executes_verified_synthetic_gguf_and_rejects_wrong_digest() -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "Bonsai-NativeDriver-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory)?;
    let artifact = DriverArtifact {
        path: directory.join("fixture.gguf"),
        directory,
    };
    let bytes = wire_model(&fixture(false))?;
    fs::write(&artifact.path, &bytes)?;
    let digest: String = Sha256::digest(&bytes)
        .into_iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|nibble| char::from(b"0123456789abcdef"[usize::from(nibble)]))
        .collect();
    for (sha, success) in [(digest, true), ("0".repeat(64), false)] {
        let output = Command::new(env!("CARGO_BIN_EXE_bonsai-prism-runtime"))
            .current_dir(&artifact.directory)
            .arg("--model")
            .arg(&artifact.path)
            .args([
                "--bytes",
                &bytes.len().to_string(),
                "--sha256",
                &sha,
                "--memory-bytes",
                "8388608",
                "--context",
                "16",
                "--checkpoint-bytes",
                "1048576",
                "--max-tokens",
                "2",
                "--tokens",
                "0,1",
            ])
            .output()?;
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if success {
            let records = String::from_utf8(output.stdout)?
                .lines()
                .map(serde_json::from_str::<Value>)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert_eq!(
                records,
                [
                    json!({"token": 1}),
                    json!({"token": 1}),
                    json!({"finish_reason":"length","prompt_tokens":2,"generated_tokens":2,"reused_tokens":0})
                ]
            );
        } else {
            assert_eq!(output.stdout, Vec::<u8>::new());
        }
    }
    Ok(())
}
