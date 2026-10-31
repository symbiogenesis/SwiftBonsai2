//! Narrow verified Swift first-projection extraction using the existing GGUF reader.
//!
//! Build only against the exact reviewed bonsai-gguf revision below. An external
//! owned-process deadline must cover full artifact hashing, extraction and cleanup.
//! No inference, transform, driver, device or unsigned-module operation is performed.
#![forbid(unsafe_code)]
#![deny(warnings)]

use bonsai_gguf::{Metadata, MetadataArray, MetadataValue, PRISM_DIALECT, VerifiedModel};
use sha2::{Digest as _, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Component, Path};
use std::process::ExitCode;

const GGUF_REVISION: &str = "7aca2251f09acb345d4afb53d5ce411f81017745";
const ARTIFACT_BYTES: u64 = 7_206_168_928;
const ARTIFACT_SHA256: &str = "5912bb739217cf25b4283a098c7b893d9baff36bdf2e72d3dbf4cb999d5633d5";
const ALL_SIGNS_SHA256: &str = "3d1ea9136a6044cd680cc1e92cde65e9dd7238e1288fe2effabec5d201c48bed";
const TENSOR: &str = "blk.0.ffn_up.weight";
const WEIGHT_BYTES: usize = 23_674_880;
const COLUMNS: usize = 5120;
const HEADER_BUDGET: usize = 128 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 8192;

type Result<T> = bonsai_gguf::Result<T>;

fn bad(message: &'static str) -> bonsai_gguf::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message).into()
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 15])
        .map(|value| char::from(b"0123456789abcdef"[usize::from(value)]))
        .collect()
}

fn digest(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes))
}

fn array<'a>(metadata: &'a Metadata, key: &str) -> Result<&'a MetadataArray> {
    metadata
        .get(key)
        .and_then(MetadataValue::as_array)
        .ok_or_else(|| bad("Missing exact Swift transform array."))
}

fn signs(metadata: &Metadata) -> Result<(Vec<u8>, String)> {
    for (key, expected) in [
        ("general.architecture", "qwen35"),
        (
            "prism.hadamard.transform",
            "normalized-sylvester-walsh-hadamard",
        ),
        ("prism.hadamard.axis", "input-last-dimension"),
        ("prism.hadamard.sign_mode", "explicit"),
    ] {
        if metadata.get(key).and_then(MetadataValue::as_str) != Some(expected) {
            return Err(bad("Changed Swift transform string."));
        }
    }
    for (key, expected) in [
        ("prism.hadamard.version", 1),
        ("prism.hadamard.block_size", 1024),
    ] {
        if metadata.get(key).and_then(MetadataValue::as_u64) != Some(expected) {
            return Err(bad("Changed Swift transform geometry."));
        }
    }
    if metadata
        .get("prism.hadamard.gdn_v_grouped")
        .and_then(MetadataValue::as_bool)
        != Some(true)
    {
        return Err(bad("Changed Swift grouped transform declaration."));
    }
    let widths = array(metadata, "prism.hadamard.sign_widths")?;
    if widths.len() != 3
        || widths
            .iter()
            .zip([5120, 6144, 17408])
            .any(|(width, expected)| width.as_u64() != Some(expected))
    {
        return Err(bad("Changed Swift sign-width ordering."));
    }
    let names = array(metadata, "prism.hadamard.weight_names")?;
    if names.len() != 401
        || names
            .iter()
            .filter(|name| name.as_str() == Some(TENSOR))
            .count()
            != 1
    {
        return Err(bad(
            "First projection lacks exactly one forward transform declaration.",
        ));
    }
    let values = array(metadata, "prism.hadamard.sign_values")?;
    if values.len() != 28_672 {
        return Err(bad("Changed Swift complete sign-array extent."));
    }
    let mut output = Vec::new();
    output.try_reserve_exact(COLUMNS)?;
    let mut hash = Sha256::new();
    for (index, value) in values.iter().enumerate() {
        let value = match value.as_i64() {
            Some(-1) => -1_i8,
            Some(1) => 1_i8,
            _ => return Err(bad("Swift signs must be integer +1 or -1.")),
        };
        // Match Prism's retained full-array identity: canonical i32 LE, before
        // extracting the first width's lossless i8 representation for the CPU.
        hash.update(i32::from(value).to_le_bytes());
        if index < COLUMNS {
            output.push(value.to_ne_bytes()[0]);
        }
    }
    Ok((output, hex(hash.finalize())))
}

fn validate_scales(weights: &[u8]) -> Result<()> {
    if !weights.len().is_multiple_of(34) {
        return Err(bad("Incomplete PQ2 block."));
    }
    for block in weights.chunks_exact(34) {
        if u16::from_le_bytes([block[0], block[1]]) & 0x7c00 == 0x7c00 {
            return Err(bad("Nonfinite original PQ2 scale."));
        }
    }
    Ok(())
}

fn external(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.components().any(|part| match part {
            Component::CurDir | Component::ParentDir => true,
            Component::Normal(name) => {
                name.to_string_lossy().contains(':')
                    || name.to_string_lossy().chars().any(char::is_control)
            }
            _ => false,
        })
    {
        return Err(bad("Expanded absolute external paths are required."));
    }
    #[cfg(windows)]
    if !matches!(path.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), std::path::Prefix::Disk(_)))
    {
        return Err(bad("Only local drive paths are supported."));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(bad("Linked external storage is unsupported."));
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt as _;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(bad("Reparse external storage is unsupported."));
                    }
                }
                if metadata.is_dir() && ancestor.join(".git").try_exists()? {
                    return Err(bad(
                        "Extraction inputs and outputs must be outside Git storage.",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.share_mode(1);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(file)
}

fn extract(model: &Path, output: &Path) -> Result<String> {
    external(model)?;
    external(output)?;
    if output.try_exists()? || !output.parent().is_some_and(Path::is_dir) {
        return Err(bad(
            "Output must be a new directory with an existing external parent.",
        ));
    }
    let mut model = VerifiedModel::open(
        model,
        PRISM_DIALECT,
        ARTIFACT_BYTES,
        ARTIFACT_SHA256,
        HEADER_BUDGET,
    )?;
    let (signs, all_signs_sha256) = signs(model.metadata())?;
    if all_signs_sha256 != ALL_SIGNS_SHA256 {
        return Err(bad("Changed Swift full sign-array identity."));
    }
    let index = model
        .tensors()
        .iter()
        .position(|tensor| tensor.name == TENSOR)
        .ok_or_else(|| bad("First projection tensor is absent."))?;
    let tensor = &model.tensors()[index];
    if tensor.type_id != 142
        || tensor.dimensions != [5120, 17408]
        || tensor.bytes != u64::try_from(WEIGHT_BYTES)?
        || tensor.elements != 89_128_960
    {
        return Err(bad("Changed first projection encoding or geometry."));
    }
    let weights = model.read_tensor(index, WEIGHT_BYTES)?;
    if weights.len() != WEIGHT_BYTES {
        return Err(bad("Changed first projection payload extent."));
    }
    validate_scales(&weights)?;
    let manifest = format!(
        "{{\"schemaVersion\":1,\"kind\":\"swift-packed-first-projection-binding\",\"artifactBytes\":{ARTIFACT_BYTES},\"artifactSha256\":\"{ARTIFACT_SHA256}\",\"artifactVerification\":\"complete-size-and-sha256-on-retained-handle\",\"ggufLibraryRevision\":\"{GGUF_REVISION}\",\"extractorSourceSha256\":\"{}\",\"tensor\":{{\"name\":\"{TENSOR}\",\"typeId\":142,\"columns\":5120,\"rows\":17408,\"bytes\":{WEIGHT_BYTES},\"file\":\"weights.pq2\",\"sha256\":\"{}\"}},\"signs\":{{\"sourceKey\":\"prism.hadamard.sign_values\",\"sourceElementStart\":0,\"count\":5120,\"completeArrayElements\":28672,\"completeArrayCanonicalI32LeSha256\":\"{ALL_SIGNS_SHA256}\",\"file\":\"signs.i8\",\"encoding\":\"twos-complement-i8\",\"sha256\":\"{}\"}},\"transform\":{{\"version\":1,\"blockSize\":1024,\"name\":\"normalized-sylvester-walsh-hadamard\",\"axis\":\"input-last-dimension\",\"signMode\":\"explicit\",\"signsAppliedBeforeRotation\":true,\"owner\":\"CPU\"}},\"headerRetentionLimitBytes\":{HEADER_BUDGET},\"retainedHeaderChargeBytes\":{},\"weightPayloadLimitBytes\":{WEIGHT_BYTES},\"maximumManifestBytes\":{MAX_MANIFEST_BYTES},\"modelInferenceExecuted\":false,\"transformExecuted\":false,\"deviceExecuted\":false,\"activationParityQualified\":false,\"logitStateParityQualified\":false,\"profileEligible\":false}}\n",
        digest(include_bytes!("packed_artifact.rs")),
        digest(&weights),
        digest(&signs),
        model.retained_bytes(),
    );
    if manifest.len() > MAX_MANIFEST_BYTES {
        return Err(bad("Binding manifest exceeds its byte bound."));
    }
    fs::create_dir(output)?;
    // Keep all output files held against mutation through final receipt creation.
    // A failure retains bounded partial files but no complete binding manifest.
    let _weights = write_new(&output.join("weights.pq2"), &weights)?;
    let _signs = write_new(&output.join("signs.i8"), &signs)?;
    let _manifest = write_new(&output.join("binding.json"), manifest.as_bytes())?;
    Ok(manifest)
}

fn self_check() -> Result<()> {
    let mut metadata = Metadata::new();
    for (key, value) in [
        ("general.architecture", "qwen35"),
        (
            "prism.hadamard.transform",
            "normalized-sylvester-walsh-hadamard",
        ),
        ("prism.hadamard.axis", "input-last-dimension"),
        ("prism.hadamard.sign_mode", "explicit"),
    ] {
        metadata.insert(key.into(), MetadataValue::String(value.into()));
    }
    metadata.insert("prism.hadamard.version".into(), MetadataValue::U32(1));
    metadata.insert("prism.hadamard.block_size".into(), MetadataValue::U32(1024));
    metadata.insert(
        "prism.hadamard.gdn_v_grouped".into(),
        MetadataValue::Bool(true),
    );
    metadata.insert(
        "prism.hadamard.sign_widths".into(),
        MetadataValue::Array(MetadataArray::U32(vec![5120, 6144, 17408])),
    );
    let mut names = vec!["other".into(); 401];
    names[200] = TENSOR.into();
    metadata.insert(
        "prism.hadamard.weight_names".into(),
        MetadataValue::Array(MetadataArray::String(names)),
    );
    let values: Vec<i32> = (0..28_672)
        .map(|index| if index % 2 == 0 { -1 } else { 1 })
        .collect();
    metadata.insert(
        "prism.hadamard.sign_values".into(),
        MetadataValue::Array(MetadataArray::I32(values)),
    );
    let (output, hash) = signs(&metadata)?;
    if output.len() != COLUMNS
        || output
            .iter()
            .enumerate()
            .any(|(index, byte)| *byte != if index % 2 == 0 { 255 } else { 1 })
        || hash == ALL_SIGNS_SHA256
    {
        return Err(bad("Synthetic sign extraction control failed."));
    }
    metadata.insert(
        "prism.hadamard.sign_widths".into(),
        MetadataValue::Array(MetadataArray::U32(vec![6144, 5120, 17408])),
    );
    if signs(&metadata).is_ok() {
        return Err(bad("Changed sign-width ordering was admitted."));
    }
    metadata.insert(
        "prism.hadamard.sign_widths".into(),
        MetadataValue::Array(MetadataArray::U32(vec![5120, 6144, 17408])),
    );
    let Some(MetadataValue::Array(MetadataArray::I32(values))) =
        metadata.get_mut("prism.hadamard.sign_values")
    else {
        return Err(bad("Synthetic sign storage changed."));
    };
    values[28_671] = 0;
    if signs(&metadata).is_ok() {
        return Err(bad("Malformed sign outside selected width was admitted."));
    }
    let mut block = [0; 34];
    block[..2].copy_from_slice(&0x8000_u16.to_le_bytes());
    validate_scales(&block)?;
    block[..2].copy_from_slice(&0x7c00_u16.to_le_bytes());
    if validate_scales(&block).is_ok() || validate_scales(&block[..33]).is_ok() {
        return Err(bad("Malformed packed scales or extents were admitted."));
    }
    Ok(())
}

fn run() -> Result<String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--self-test" {
        self_check()?;
        return Ok("{\"schemaVersion\":1,\"kind\":\"packed-artifact-self-check\",\"passed\":true,\"modelRead\":false,\"deviceExecuted\":false,\"profileEligible\":false}\n".into());
    }
    if args.len() != 4 || args[0] != "--model" || args[2] != "--output-directory" {
        return Err(bad(
            "Use --self-test or --model ABSOLUTE_GGUF --output-directory NEW_EXTERNAL_DIRECTORY.",
        ));
    }
    extract(Path::new(&args[1]), Path::new(&args[3]))
}

fn main() -> ExitCode {
    match run() {
        Ok(receipt) => match std::io::stdout().lock().write_all(receipt.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(_) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "Packed artifact binding rejected; incomplete files remain pending."
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fixed_transform_sign_selection_and_malformed_control() -> super::Result<()> {
        super::self_check()
    }
}
