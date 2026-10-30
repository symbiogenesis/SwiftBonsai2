//! Offline verification of compiled model/runtime identities with retained guards.

use crate::launch_policy::{LaunchProfile, reviewed_pins_for_representation};
use bonsai_artifacts::{RuntimeSpec, VerifiedFile, VerifiedRuntime};
use std::path::{Path, PathBuf};

pub use bonsai_artifacts::ArtifactError;

/// Verified policy paths and guards retained until the backend child exits.
pub struct VerifiedArtifacts {
    executable: PathBuf,
    model: PathBuf,
    runtime_directory: PathBuf,
    archive_path: PathBuf,
    // Retain archive/installation handles through the Windows backend lifetime.
    // The non-Windows verifier rejects before acquiring any runtime guard.
    #[cfg(windows)]
    runtime: Option<VerifiedRuntime>,
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
            runtime: None,
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
        Self::verify_for_profile_with_representation(data, profile, false)
    }

    /// Verify one compiled model/runtime profile and its explicitly selected weight representation.
    /// PTQ1 uses the pinned current-weight export receipt and retains the same runtime verification.
    ///
    /// # Errors
    /// Rejects incompatible selection, policy/path drift, changed artifacts and unsafe installations.
    pub fn verify_for_profile_with_representation(
        data: &Path,
        profile: LaunchProfile,
        use_repacked_ptq1: bool,
    ) -> Result<Self, ArtifactError> {
        if !cfg!(windows) {
            return Err(ArtifactError::Configuration);
        }
        let pins = reviewed_pins_for_representation(profile, use_repacked_ptq1)
            .map_err(|_| ArtifactError::Configuration)?;
        let spec = RuntimeSpec {
            archive_path: data.join("downloads").join(&pins.runtime.asset.filename),
            runtime_directory: data.join("runtime").join(&pins.runtime.commit),
            archive_bytes: pins.runtime.asset.bytes,
            archive_sha256: pins.runtime.asset.sha256.clone(),
        };
        let runtime = VerifiedRuntime::verify(&spec)?;
        let (model, model_guard) = Self::verify_selected_model(data, profile, use_repacked_ptq1)?;
        Ok(Self {
            executable: runtime.executable().to_owned(),
            model,
            runtime_directory: spec.runtime_directory,
            archive_path: spec.archive_path,
            #[cfg(windows)]
            runtime: Some(runtime),
            _model: Some(model_guard),
        })
    }

    #[cfg(windows)]
    pub(crate) fn verify_model_for_profile(
        data: &Path,
        profile: LaunchProfile,
        use_repacked_ptq1: bool,
    ) -> Result<Self, ArtifactError> {
        let (model, model_guard) = Self::verify_selected_model(data, profile, use_repacked_ptq1)?;
        Ok(Self {
            executable: PathBuf::new(),
            model,
            runtime_directory: PathBuf::new(),
            archive_path: PathBuf::new(),
            runtime: None,
            _model: Some(model_guard),
        })
    }

    fn verify_selected_model(
        data: &Path,
        profile: LaunchProfile,
        use_repacked_ptq1: bool,
    ) -> Result<(PathBuf, VerifiedFile), ArtifactError> {
        let pins = reviewed_pins_for_representation(profile, use_repacked_ptq1)
            .map_err(|_| ArtifactError::Configuration)?;
        let model = data
            .join("models")
            .join(&pins.model.revision)
            .join(&pins.model.filename);
        let guard = VerifiedFile::verify(&model, pins.model.bytes, &pins.model.sha256)?;
        Ok((model, guard))
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
    /// Exact installed runtime files established by the retained verifier.
    #[cfg(windows)]
    pub(crate) fn runtime_files(&self) -> Vec<PathBuf> {
        self.runtime
            .iter()
            .flat_map(VerifiedRuntime::files)
            .map(|file| file.path().to_owned())
            .collect()
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
    use crate::launch_policy::reviewed_pins;
    use std::fs;

    #[test]
    fn ptq1_verification_rejects_missing_or_altered_selected_file_without_pq2_fallback()
    -> std::io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir();
        let data = parent.join(format!(
            "Bonsai-ptq1-identity-{}-{stamp}",
            std::process::id()
        ));
        let profile = LaunchProfile::SwiftPq2Assessment;
        let pins = reviewed_pins(profile).map_err(std::io::Error::other)?;
        let directory = data.join("models").join(&pins.model.revision);
        fs::create_dir_all(&directory)?;
        let pq2 = directory.join(pins.model.filename);
        let ptq1 = directory.join("Swift-Bonsai-2-current-repacked-PTQ1_0.gguf");
        fs::write(&ptq1, b"altered PTQ1")?;
        assert_eq!(
            VerifiedArtifacts::verify_selected_model(&data, profile, true).err(),
            Some(ArtifactError::Identity)
        );
        assert_eq!(
            VerifiedArtifacts::verify_selected_model(&data, profile, false).err(),
            Some(ArtifactError::Path)
        );
        fs::write(&pq2, b"altered PQ2")?;
        fs::remove_file(&ptq1)?;
        assert_eq!(
            VerifiedArtifacts::verify_selected_model(&data, profile, true).err(),
            Some(ArtifactError::Path)
        );
        assert_eq!(
            VerifiedArtifacts::verify_selected_model(&data, profile, false).err(),
            Some(ArtifactError::Identity)
        );
        assert_eq!(data.parent(), Some(parent.as_path()));
        fs::remove_dir_all(data)?;
        Ok(())
    }

    #[test]
    fn native_profile_checks_model_without_requiring_upstream_runtime() -> std::io::Result<()> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let parent = std::env::temp_dir();
        let data = parent.join(format!(
            "Bonsai-native-model-{}-{stamp}",
            std::process::id()
        ));
        let profile = LaunchProfile::SwiftPq2Assessment;
        let pins = reviewed_pins(profile).map_err(std::io::Error::other)?;
        let model_directory = data.join("models").join(&pins.model.revision);
        fs::create_dir_all(&model_directory)?;
        let model = model_directory.join(&pins.model.filename);
        fs::write(&model, b"")?;
        // Identity, rather than Path, proves admission reached the model without
        // first opening a C++ archive or requiring its installed runtime tree.
        assert_eq!(
            VerifiedArtifacts::verify_model_for_profile(&data, profile, false).err(),
            Some(ArtifactError::Identity)
        );
        assert!(!data.join("downloads").exists());
        fs::remove_file(model)?;
        fs::remove_dir(&model_directory)?;
        fs::remove_dir(data.join("models"))?;
        assert_eq!(data.parent(), Some(parent.as_path()));
        fs::remove_dir(data)?;
        Ok(())
    }

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
