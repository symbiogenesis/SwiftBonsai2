//! Exact readiness for the explicitly selected native representation and adapter.

use serde::{Deserialize, Deserializer};
use std::io;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Models {
    data: Vec<crate::Object<NativeModel>>,
}

#[derive(Default)]
enum AdapterIdentity {
    #[default]
    Absent,
    Present(Option<String>),
}

impl<'de> Deserialize<'de> for AdapterIdentity {
    fn deserialize<D: Deserializer<'de>>(input: D) -> Result<Self, D::Error> {
        Option::<String>::deserialize(input).map(Self::Present)
    }
}

fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    input: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(input).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeModel {
    id: String,
    object: String,
    model_sha256: String,
    template_sha256: String,
    runtime: String,
    context_tokens: usize,
    seed: u32,
    #[serde(default, deserialize_with = "present")]
    weight_representation: Option<String>,
    #[serde(default)]
    adapter_sha256: AdapterIdentity,
    #[serde(default, deserialize_with = "present")]
    supports_abliteration_scale: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    abliteration_scale_range: Option<[u8; 2]>,
}

pub(super) fn validate_native_identity(
    bytes: &[u8],
    repacked: bool,
    adapter: bool,
) -> io::Result<()> {
    let body = super::validate_response(bytes, 200)?;
    let crate::Object(models): crate::Object<Models> =
        serde_json::from_slice(body).map_err(io::Error::other)?;
    let [crate::Object(model)] = models.data.as_slice() else {
        return Err(io::Error::other("native model inventory mismatch"));
    };
    let (id, hash, representation) = if repacked {
        (
            "Swift-Bonsai-2-current-repacked-PTQ1_0",
            crate::inference::REPACKED_PTQ1_SHA256,
            "current-repacked-PTQ1_0",
        )
    } else {
        (
            "Swift-Bonsai-2-PQ2_0",
            crate::inference::MODEL_SHA256,
            "PQ2_0",
        )
    };
    let extensions_match = if repacked || adapter {
        model.weight_representation.as_deref() == Some(representation)
            && matches!(&model.adapter_sha256, AdapterIdentity::Present(value)
                if value.as_deref() == adapter.then_some(crate::inference::ADAPTER_SHA256))
            && model.supports_abliteration_scale == Some(adapter)
            && model.abliteration_scale_range == adapter.then_some([0, 2])
    } else {
        model.weight_representation.is_none()
            && matches!(&model.adapter_sha256, AdapterIdentity::Absent)
            && model.supports_abliteration_scale.is_none()
            && model.abliteration_scale_range.is_none()
    };
    if model.id != id
        || model.object != "model"
        || model.model_sha256 != hash
        || model.template_sha256 != crate::inference::TEMPLATE_SHA256
        || model.runtime != "prism-inference"
        || model.context_tokens != crate::CONTEXT_TOKENS
        || model.seed != 42
        || !extensions_match
    {
        return Err(io::Error::other("native model readiness identity mismatch"));
    }
    Ok(())
}
