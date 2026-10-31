//! Apply real Rust planning to the generated native graph-lifetime fixture.

use std::{
    env,
    error::Error,
    ffi::{c_char, c_void},
    fs, io,
    os::windows::ffi::OsStrExt as _,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use super::decode_graph::{bonsai_qt_decode_graph_index, bonsai_qt_decode_graph_rebuild};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

struct FixtureLibrary(*mut c_void);

impl Drop for FixtureLibrary {
    fn drop(&mut self) {
        // SAFETY: The guard owns one successfully loaded module reference. All
        // borrowed symbols and native calls have ended before this drop.
        unsafe { FreeLibrary(self.0) };
    }
}

type RunFixture = unsafe extern "C" fn(
    extern "C" fn(i32, usize) -> usize,
    extern "C" fn(bool, i32, i32) -> bool,
) -> i32;

#[test]
fn generated_decode_workspace_eviction_releases_only_completed_graphs() -> Result<(), Box<dyn Error>>
{
    let compiler = PathBuf::from(
        env::var_os("BONSAI_TTS_COMPILER")
            .or_else(|| option_env!("BONSAI_TTS_COMPILER").map(Into::into))
            .ok_or("Set the provisioned BONSAI_TTS_COMPILER for the native cache regression.")?,
    );
    assert!(compiler.is_absolute() && compiler.is_file());
    let working = env::temp_dir().join(format!(
        "Bonsai-Qwen-GraphCache-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
    ));
    assert!(working.is_absolute() && !working.to_string_lossy().contains('%'));
    fs::create_dir(&working)?;
    let source = working.join("cache.cpp");
    let library = working.join("cache.dll");
    fs::write(
        &source,
        crate::build_overlay::decode_graph_fixture_for_test()?,
    )?;
    let output = Command::new(compiler)
        .args([
            "/nologo",
            "/std:c++17",
            "/EHsc",
            "/W4",
            "/WX",
            "/UNDEBUG",
            "/LD",
        ])
        .arg(&source)
        .arg(format!("/Fe{}", library.display()))
        .current_dir(&working)
        .output()?;
    fs::write(working.join("compiler.stdout"), &output.stdout)?;
    fs::write(working.join("compiler.stderr"), &output.stderr)?;
    assert!(
        output.status.success(),
        "Native cache fixture failed; retained {}",
        working.display()
    );

    let path = library
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    // SAFETY: The absolute, freshly compiled fixture path is NUL-terminated.
    // Search is limited to the DLL directory and standard safe DLL directories.
    let module = unsafe { LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), 0x1100) };
    if module.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    let guard = FixtureLibrary(module);
    // SAFETY: The guard keeps this fresh fixture module loaded; the literal is
    // NUL-terminated and names its one explicitly exported C entry point.
    let address = unsafe { GetProcAddress(guard.0, c"bonsai_decode_graph_fixture".as_ptr()) };
    if address.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: The freshly compiled source defines exactly RunFixture's C ABI.
    let run: RunFixture = unsafe { std::mem::transmute(address) };
    // SAFETY: Both pointer-free Rust exports match the fixture callback types
    // and remain live throughout its synchronous, assertion-enabled invocation.
    assert_eq!(
        unsafe { run(bonsai_qt_decode_graph_index, bonsai_qt_decode_graph_rebuild) },
        0
    );
    drop(guard);
    fs::remove_dir_all(working)?;
    Ok(())
}
