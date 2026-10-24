//! Offline verification of compiled model/runtime identities with retained guards.

use crate::launch_policy::{LaunchProfile, reviewed_pins};
use bonsai_artifacts::{RuntimeSpec, VerifiedFile, VerifiedRuntime};
use std::path::{Path, PathBuf};

pub use bonsai_artifacts::ArtifactError;

/// Verified policy paths and guards retained until the backend child exits.
pub struct VerifiedArtifacts {
    executable: PathBuf,
    model: PathBuf,
    runtime_directory: PathBuf,
    archive_path: PathBuf,
    _runtime: Option<VerifiedRuntime>,
    _model: Option<VerifiedFile>,
}
impl VerifiedArtifacts {
    #[cfg(all(test, windows))]
    pub(crate) const fn synthetic_for_test() -> Self {
        Self {
            executable: PathBuf::new(),
            model: PathBuf::new(),
            runtime_directory: PathBuf::new(),
            archive_path: PathBuf::new(),
            _runtime: None,
            _model: None,
        }
    }

    /// Verify the locked reference model and runtime, retaining their read handles.
    ///
    /// # Errors
    /// Rejects policy/path drift, changed artifacts and unsafe installations.
    pub fn verify(data: &Path) -> Result<Self, ArtifactError> {
        Self::verify_for_profile(data, LaunchProfile::ReferenceCpu)
    }

    /// Verify one compiled model/runtime profile through the shared verifier.
    ///
    /// # Errors
    /// Rejects policy/path drift, changed artifacts and unsafe installations.
    pub fn verify_for_profile(data: &Path, profile: LaunchProfile) -> Result<Self, ArtifactError> {
        if !cfg!(windows) {
            return Err(ArtifactError::Configuration);
        }
        let pins = reviewed_pins(profile).map_err(|_| ArtifactError::Configuration)?;
        let spec = RuntimeSpec {
            archive_path: data.join("downloads").join(&pins.runtime.asset.filename),
            runtime_directory: data.join("runtime").join(&pins.runtime.commit),
            archive_bytes: pins.runtime.asset.bytes,
            archive_sha256: pins.runtime.asset.sha256.clone(),
        };
        let runtime = VerifiedRuntime::verify(&spec)?;
        let model = data
            .join("models")
            .join(&pins.model.revision)
            .join(&pins.model.filename);
        let model_guard = VerifiedFile::verify(&model, pins.model.bytes, &pins.model.sha256)?;
        Ok(Self {
            executable: runtime.executable().to_owned(),
            model,
            runtime_directory: spec.runtime_directory,
            archive_path: spec.archive_path,
            _runtime: Some(runtime),
            _model: Some(model_guard),
        })
    }

    /// Server path established by the verified archive.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }
    /// Whole-model path held stable by verification handles.
    #[must_use]
    pub fn model(&self) -> &Path {
        &self.model
    }
    /// Complete installed runtime directory.
    #[must_use]
    pub fn runtime_directory(&self) -> &Path {
        &self.runtime_directory
    }
    /// Locked runtime ZIP path.
    #[must_use]
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(windows)]
    #[test]
    fn profile_verification_opens_only_its_locked_archive() -> std::io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir();
        let data = parent.join(format!(
            "Bonsai-artifact-profile-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&data)?;
        fs::create_dir(data.join("downloads"))?;
        for profile in [
            LaunchProfile::ReferenceCpu,
            LaunchProfile::SwiftPq2Assessment,
        ] {
            let pins = reviewed_pins(profile).map_err(std::io::Error::other)?;
            fs::create_dir_all(data.join("runtime").join(&pins.runtime.commit))?;
            fs::create_dir_all(data.join("models").join(&pins.model.revision))?;
        }
        for selected in [
            LaunchProfile::ReferenceCpu,
            LaunchProfile::SwiftPq2Assessment,
        ] {
            let pins = reviewed_pins(selected).map_err(std::io::Error::other)?;
            let archive = data.join("downloads").join(&pins.runtime.asset.filename);
            fs::write(&archive, b"")?;
            for candidate in [
                LaunchProfile::ReferenceCpu,
                LaunchProfile::SwiftPq2Assessment,
            ] {
                let expected = if candidate == selected {
                    ArtifactError::Identity
                } else {
                    ArtifactError::Path
                };
                assert_eq!(
                    VerifiedArtifacts::verify_for_profile(&data, candidate).err(),
                    Some(expected)
                );
            }
            assert_eq!(
                VerifiedArtifacts::verify(&data).err(),
                Some(if selected == LaunchProfile::ReferenceCpu {
                    ArtifactError::Identity
                } else {
                    ArtifactError::Path
                })
            );
            fs::remove_file(&archive)?;
        }
        assert_eq!(data.parent(), Some(parent.as_path()));
        fs::remove_dir_all(data)?;
        Ok(())
    }
}
