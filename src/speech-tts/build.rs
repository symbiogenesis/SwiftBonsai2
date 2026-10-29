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
    time::{SystemTime, UNIX_EPOCH},
};

mod build_overlay;

fn main() -> io::Result<()> {
    for name in [
        "BONSAI_TTS_SOURCE_DIR",
        "BONSAI_TTS_NATIVE_BUILD_DIR",
        "BONSAI_TTS_COMPILER",
        "BONSAI_CMAKE",
        "BONSAI_CMAKE_GENERATOR",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=model.lock.json");
    println!("cargo:rerun-if-changed=build_overlay.rs");
    println!("cargo:rerun-if-changed=tests/reference/philox.h");
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
        .unwrap_or_else(|| OsString::from("Ninja Multi-Config"));
    // Native tools and SDK helpers may still have MAX_PATH limits even when
    // Windows long paths are enabled. Compile in a short external temp lane,
    // then retain the libraries in Cargo's own output for subsequent links.
    let native_work = native_work()?;
    fs::write(
        output.join("native-build-location.txt"),
        native_work.as_os_str().to_string_lossy().as_bytes(),
    )?;
    let engine_build = engine_build(&source, &native_work, &cmake, &generator)?;
    external_output(&engine_build)?;
    println!("cargo:rustc-link-search=native={}", output.display());
    for (name, path) in [
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
        fs::copy(&path, output.join(format!("{name}.lib")))?;
        println!("cargo:rustc-link-lib=static={name}");
    }
    // GGML queries the Windows CPU description through the registry API.
    println!("cargo:rustc-link-lib=advapi32");
    Ok(())
}

fn native_work() -> io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("Cannot create native speech build evidence."))?
        .as_nanos();
    let path = env::temp_dir().join(format!("BQ-{}-{stamp:x}", std::process::id()));
    if !path.is_absolute() || path.as_os_str().to_string_lossy().contains('%') {
        return Err(io::Error::other(
            "Use an expanded absolute system temp path.",
        ));
    }
    external_output(&path)?;
    fs::create_dir(&path)?;
    Ok(path)
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
        if generator != "Ninja Multi-Config" {
            return Err(io::Error::other(
                "The native Qwen build requires Ninja Multi-Config and pinned ARM64 clang-cl.",
            ));
        }
        let compiler = absolute_env("BONSAI_TTS_COMPILER")?;
        let resource_compiler = compiler
            .parent()
            .ok_or_else(|| io::Error::other("Missing native speech compiler directory."))?
            .join("llvm-rc.exe");
        if compiler
            .file_name()
            .is_none_or(|name| name != "clang-cl.exe")
            || !compiler.is_file()
            || !resource_compiler.is_file()
        {
            return Err(io::Error::other(
                "Provision the pinned ARM64 clang-cl and LLVM resource compiler before building Qwen.",
            ));
        }
        let private_source = output.join("qwen-source");
        build_overlay::prepare(source, &private_source)?;
        let directory = output.join("qwen-build");
        fs::create_dir_all(&directory)?;
        let mut configure = Command::new(cmake);
        configure.args([
            OsString::from("-S"),
            private_source.into_os_string(),
            OsString::from("-B"),
            directory.clone().into_os_string(),
            OsString::from("-G"),
            generator.to_owned(),
        ]);
        let compiler_argument = compiler.to_string_lossy().replace('\\', "/");
        for name in ["CMAKE_C_COMPILER", "CMAKE_CXX_COMPILER"] {
            configure.arg(format!("-D{name}:FILEPATH={compiler_argument}"));
        }
        configure.arg(format!(
            "-DCMAKE_RC_COMPILER:FILEPATH={}",
            resource_compiler.to_string_lossy().replace('\\', "/")
        ));
        configure.args([
            "-DCMAKE_C_COMPILER_TARGET=aarch64-pc-windows-msvc",
            "-DCMAKE_CXX_COMPILER_TARGET=aarch64-pc-windows-msvc",
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
    // compiler selection is explicit in CMake arguments. Inherited overrides
    // for another native package must not replace the verified ARM64 toolset.
    for key in [
        "CC",
        "CXX",
        "CMAKE_C_COMPILER",
        "CMAKE_CXX_COMPILER",
        "CMAKE_C_COMPILER_TARGET",
        "CMAKE_CXX_COMPILER_TARGET",
        "CMAKE_GENERATOR_TOOLSET",
        "CMAKE_TOOLCHAIN_FILE",
        "RC",
        "CL",
    ] {
        command.env_remove(key);
    }
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
