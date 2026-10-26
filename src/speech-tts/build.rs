//! Build the reviewed, externally provisioned native Qwen static libraries.
#![allow(
    clippy::print_stdout,
    reason = "Cargo build-script directives are the required communication protocol."
)]

use std::{
    env,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn main() -> io::Result<()> {
    for name in [
        "BONSAI_TTS_SOURCE_DIR",
        "BONSAI_TTS_NATIVE_BUILD_DIR",
        "BONSAI_CMAKE",
        "BONSAI_CMAKE_GENERATOR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for path in [
        "native/bridge.cpp",
        "native/CMakeLists.txt",
        "model.lock.json",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|value| value != "windows") {
        return Ok(());
    }
    if !env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|value| value == "aarch64") {
        return Err(io::Error::other(
            "The native Qwen build requires Windows ARM64.",
        ));
    }
    let source = absolute_env("BONSAI_TTS_SOURCE_DIR")?;
    for file in ["src/qwen.h", "src/qwen.cpp", "ggml/CMakeLists.txt"] {
        if !source.join(file).is_file() {
            return Err(io::Error::other(
                "Provision the pinned Qwen and GGML source archives before building.",
            ));
        }
    }
    let output = absolute_env("OUT_DIR")?;
    external_output(&output)?;
    let cmake = env::var_os("BONSAI_CMAKE").unwrap_or_else(|| OsString::from("cmake"));
    let generator = env::var_os("BONSAI_CMAKE_GENERATOR")
        .unwrap_or_else(|| OsString::from("Visual Studio 18 2026"));
    let engine_build = engine_build(&source, &output, &cmake, &generator)?;
    external_output(&engine_build)?;
    let manifest = absolute_env("CARGO_MANIFEST_DIR")?;
    let bridge = output.join("bridge-build");
    fs::create_dir_all(&bridge)?;
    let mut configure = Command::new(&cmake);
    configure.args([
        OsString::from("-S"),
        manifest.join("native").into_os_string(),
        OsString::from("-B"),
        bridge.clone().into_os_string(),
        OsString::from("-G"),
        generator,
        OsString::from("-A"),
        OsString::from("ARM64"),
        OsString::from(format!("-DBONSAI_TTS_SOURCE_DIR={}", source.display())),
        OsString::from("-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL"),
    ]);
    run(&mut configure, &output)?;
    build(&cmake, &bridge, "bonsai-qwen-bridge", &output)?;
    for (name, path) in [
        (
            "bonsai-qwen-bridge",
            bridge.join("Release/bonsai-qwen-bridge.lib"),
        ),
        ("qwen-core", engine_build.join("Release/qwen-core.lib")),
        ("ggml", engine_build.join("ggml/src/Release/ggml.lib")),
        (
            "ggml-base",
            engine_build.join("ggml/src/Release/ggml-base.lib"),
        ),
        (
            "ggml-cpu",
            engine_build.join("ggml/src/Release/ggml-cpu.lib"),
        ),
    ] {
        if !path.is_file() {
            return Err(io::Error::other(
                "A required native speech static library was not built.",
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Invalid native library path."))?;
        println!("cargo:rustc-link-search=native={}", parent.display());
        println!("cargo:rustc-link-lib=static={name}");
    }
    Ok(())
}

fn engine_build(
    source: &Path,
    output: &Path,
    cmake: &std::ffi::OsStr,
    generator: &std::ffi::OsStr,
) -> io::Result<PathBuf> {
    let directory = if env::var_os("BONSAI_TTS_NATIVE_BUILD_DIR").is_some() {
        absolute_env("BONSAI_TTS_NATIVE_BUILD_DIR")?
    } else {
        let directory = output.join("qwen-build");
        fs::create_dir_all(&directory)?;
        let mut configure = Command::new(cmake);
        configure.args([
            OsString::from("-S"),
            source.as_os_str().to_owned(),
            OsString::from("-B"),
            directory.clone().into_os_string(),
            OsString::from("-G"),
            generator.to_owned(),
            OsString::from("-A"),
            OsString::from("ARM64"),
        ]);
        configure.args([
            "-DBUILD_SHARED_LIBS=OFF",
            "-DGGML_BACKEND_DL=OFF",
            "-DGGML_NATIVE=ON",
            "-DGGML_OPENMP=OFF",
            "-DGGML_CPU_KLEIDIAI=OFF",
            "-DGGML_CUDA=OFF",
            "-DGGML_VULKAN=OFF",
            "-DGGML_METAL=OFF",
            "-DGGML_BLAS=OFF",
            "-DGGML_BUILD_TESTS=OFF",
            "-DGGML_BUILD_EXAMPLES=OFF",
            "-DQWEN_SHARED=OFF",
            "-DGGML_CCACHE=OFF",
            "-DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreadedDLL",
        ]);
        run(&mut configure, output)?;
        build(cmake, &directory, "qwen-core", output)?;
        directory
    };
    Ok(directory)
}

fn absolute_env(name: &str) -> io::Result<PathBuf> {
    let value = env::var_os(name).ok_or_else(|| {
        io::Error::other(format!(
            "Set {name} to an explicitly provisioned absolute path."
        ))
    })?;
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.as_os_str().to_string_lossy().contains('%') {
        return Err(io::Error::other(
            "Native speech build paths must be absolute and expanded.",
        ));
    }
    Ok(path)
}

fn external_output(path: &Path) -> io::Result<()> {
    if path.ancestors().any(|parent| parent.join(".git").exists()) {
        return Err(io::Error::other(
            "Native speech build outputs must remain outside Git.",
        ));
    }
    Ok(())
}

fn run(command: &mut Command, working: &Path) -> io::Result<()> {
    // Preserve the inherited Windows and profile environment; only the native
    // current directory is selected for compiler probes and diagnostics.
    let status = command.current_dir(working).status()?;
    if !status.success() {
        return Err(io::Error::other(
            "The pinned native speech compiler command failed.",
        ));
    }
    Ok(())
}

fn build(
    cmake: &std::ffi::OsStr,
    directory: &Path,
    target: &str,
    working: &Path,
) -> io::Result<()> {
    run(
        Command::new(cmake).args([
            OsString::from("--build"),
            directory.as_os_str().to_owned(),
            OsString::from("--config"),
            OsString::from("Release"),
            OsString::from("--target"),
            OsString::from(target),
            OsString::from("--parallel"),
            OsString::from("4"),
        ]),
        working,
    )
}
