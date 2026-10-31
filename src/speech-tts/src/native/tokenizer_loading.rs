//! GGUF metadata access and transactional tokenizer ownership in Rust.
//!
//! The external GGML reader still parses the verified model. Its primitive C
//! declarations match ggml cc5df49a6b878db2b68b9a7b45976e279594627d/include/gguf.h.

use std::{
    ffi::{CStr, c_char, c_int, c_void},
    io,
    ptr::NonNull,
};

use super::{Owner, guard, new_owner, owner_mut, reject_stage};

const TOKENS: &CStr = c"tokenizer.ggml.tokens";
const MERGES: &CStr = c"tokenizer.ggml.merges";
const MAX_METADATA_ENTRIES: usize = 1_000_000;
const ARRAY: c_int = 9;
const STRING: c_int = 8;
const UINT32: c_int = 4;

#[repr(C)]
#[derive(Clone, Copy)]
struct InitParams {
    no_alloc: bool,
    context: *mut *mut c_void,
}

unsafe extern "C" {
    fn gguf_init_from_file(path: *const c_char, params: InitParams) -> *mut c_void;
    fn gguf_free(context: *mut c_void);
    fn gguf_find_key(context: *const c_void, name: *const c_char) -> i64;
    fn gguf_get_kv_type(context: *const c_void, key: i64) -> c_int;
    fn gguf_get_arr_type(context: *const c_void, key: i64) -> c_int;
    fn gguf_get_arr_n(context: *const c_void, key: i64) -> usize;
    fn gguf_get_arr_str(context: *const c_void, key: i64, index: usize) -> *const c_char;
    fn gguf_get_val_u32(context: *const c_void, key: i64) -> u32;
}

struct Api {
    open: unsafe extern "C" fn(*const c_char, InitParams) -> *mut c_void,
    close: unsafe extern "C" fn(*mut c_void),
    find: unsafe extern "C" fn(*const c_void, *const c_char) -> i64,
    kind: unsafe extern "C" fn(*const c_void, i64) -> c_int,
    array_kind: unsafe extern "C" fn(*const c_void, i64) -> c_int,
    count: unsafe extern "C" fn(*const c_void, i64) -> usize,
    string: unsafe extern "C" fn(*const c_void, i64, usize) -> *const c_char,
    u32_value: unsafe extern "C" fn(*const c_void, i64) -> u32,
}

const NATIVE_API: Api = Api {
    open: gguf_init_from_file,
    close: gguf_free,
    find: gguf_find_key,
    kind: gguf_get_kv_type,
    array_kind: gguf_get_arr_type,
    count: gguf_get_arr_n,
    string: gguf_get_arr_str,
    u32_value: gguf_get_val_u32,
};

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid speech tokenizer metadata.",
    )
}

trait Metadata {
    fn find(&self, name: &CStr) -> Option<i64>;
    fn string_count(&self, key: i64) -> io::Result<usize>;
    fn string(&self, key: i64, index: usize) -> io::Result<&[u8]>;
    fn u32_value(&self, key: i64) -> io::Result<u32>;
}

struct Gguf<'a> {
    context: NonNull<c_void>,
    api: &'a Api,
}

impl<'a> Gguf<'a> {
    fn open(path: &CStr, api: &'a Api) -> io::Result<Self> {
        // SAFETY: The pinned C reader synchronously borrows a NUL-terminated
        // path. no_alloc=true and a null context request metadata only. A
        // successful return transfers one context to this Rust Drop guard.
        let raw = unsafe {
            (api.open)(
                path.as_ptr(),
                InitParams {
                    no_alloc: true,
                    context: std::ptr::null_mut(),
                },
            )
        };
        Ok(Self {
            context: NonNull::new(raw).ok_or_else(invalid)?,
            api,
        })
    }
}

impl Drop for Gguf<'_> {
    fn drop(&mut self) {
        // SAFETY: This guard uniquely owns the nonnull context from api.open;
        // all metadata borrows end before its one matching release.
        unsafe { (self.api.close)(self.context.as_ptr()) };
    }
}

impl Metadata for Gguf<'_> {
    fn find(&self, name: &CStr) -> Option<i64> {
        // SAFETY: The immutable context and NUL-terminated key remain live for
        // this synchronous call. GGML returns a valid index or -1.
        let key = unsafe { (self.api.find)(self.context.as_ptr(), name.as_ptr()) };
        (key >= 0).then_some(key)
    }

    fn string_count(&self, key: i64) -> io::Result<usize> {
        // SAFETY: key was returned by find on this live immutable context.
        let kind = unsafe { (self.api.kind)(self.context.as_ptr(), key) };
        if kind != ARRAY {
            return Err(invalid());
        }
        // SAFETY: The key is an existing ARRAY; query its element type before
        // calling GGML getters, which abort on a mismatched type.
        let element = unsafe { (self.api.array_kind)(self.context.as_ptr(), key) };
        if element != STRING {
            return Err(invalid());
        }
        // SAFETY: Both array and string element types were checked above.
        Ok(unsafe { (self.api.count)(self.context.as_ptr(), key) })
    }

    fn string(&self, key: i64, index: usize) -> io::Result<&[u8]> {
        // SAFETY: The loader queries string_count before iterating exactly that
        // extent; the immutable array's type and index are already checked.
        let pointer = unsafe { (self.api.string)(self.context.as_ptr(), key, index) };
        if pointer.is_null() {
            return Err(invalid());
        }
        // SAFETY: Pinned GGML returns an array-owned NUL-terminated string.
        // The returned borrow cannot outlive this context. Like the retired
        // std::strlen handoff, bytes after an embedded NUL are not inserted.
        Ok(unsafe { CStr::from_ptr(pointer) }.to_bytes())
    }

    fn u32_value(&self, key: i64) -> io::Result<u32> {
        // SAFETY: key was returned by find on this context.
        let kind = unsafe { (self.api.kind)(self.context.as_ptr(), key) };
        if kind != UINT32 {
            return Err(invalid());
        }
        // SAFETY: The key's UINT32 type was checked before the typed getter.
        Ok(unsafe { (self.api.u32_value)(self.context.as_ptr(), key) })
    }
}

fn candidate(metadata: &impl Metadata) -> io::Result<Box<Owner>> {
    let token_key = metadata.find(TOKENS).ok_or_else(invalid)?;
    let merge_key = metadata.find(MERGES).ok_or_else(invalid)?;
    let tokens = metadata.string_count(token_key)?;
    let merges = metadata.string_count(merge_key)?;
    if tokens > MAX_METADATA_ENTRIES || merges > MAX_METADATA_ENTRIES {
        return Err(invalid());
    }
    let mut owner = new_owner()?;
    for index in 0..tokens {
        reject_stage(2)?;
        owner.tokenizer.add_token(
            metadata.string(token_key, index)?,
            i32::try_from(index).map_err(io::Error::other)?,
        )?;
    }
    for index in 0..merges {
        reject_stage(3)?;
        owner.tokenizer.add_merge(
            metadata.string(merge_key, index)?,
            i32::try_from(index).map_err(io::Error::other)?,
        )?;
    }
    reject_stage(4)?;
    owner.tokenizer.finish()?;
    Ok(owner)
}

fn reload(slot: &mut *mut c_void, path: &CStr, api: &Api) -> io::Result<()> {
    let metadata = Gguf::open(path, api)?;
    let replacement = candidate(&metadata)?;
    let old = std::mem::replace(slot, Box::into_raw(replacement).cast());
    if !old.is_null() {
        // SAFETY: The private native wrapper retains exactly one original
        // Box<Owner> pointer in this slot, without outstanding borrows. Publish
        // only the finalized replacement, then release the displaced owner.
        drop(unsafe { Box::from_raw(old.cast::<Owner>()) });
    }
    Ok(())
}

fn specials(
    owner: &mut crate::tokenizer::Tokenizer,
    metadata: &impl Metadata,
    keys: impl IntoIterator<Item = impl AsRef<CStr>>,
) -> io::Result<()> {
    for name in keys {
        if let Some(key) = metadata.find(name.as_ref()) {
            let bits = metadata.u32_value(key)?;
            reject_stage(6)?;
            let id = i32::from_le_bytes(bits.to_le_bytes());
            #[cfg(test)]
            tests::SPECIAL_IDS.with(|ids| ids.borrow_mut().push(id));
            owner.add_special_id(id)?;
        }
    }
    Ok(())
}

// SAFETY: The generated noncopyable native object supplies an exclusively
// borrowed owner slot containing null or one live Box<Owner> pointer, plus a
// NUL-terminated path. Success replaces that slot; failure leaves it unchanged.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_load_gguf(
    slot: *mut *mut c_void,
    path: *const c_char,
) -> bool {
    guard(|| {
        if slot.is_null() || !slot.is_aligned() || path.is_null() {
            return false;
        }
        // SAFETY: The generated object provides one exclusively borrowed live
        // pointer slot. No other references to its value overlap this call.
        let slot = unsafe { &mut *slot };
        if !slot.is_null() && !slot.cast::<Owner>().is_aligned() {
            return false;
        }
        // SAFETY: The native caller borrows its NUL-terminated path until return.
        let path = unsafe { CStr::from_ptr(path) };
        reload(slot, path, &NATIVE_API).is_ok()
    })
    .unwrap_or(false)
}

// SAFETY: The generated wrapper borrows a live exclusive Box<Owner>, a
// NUL-terminated path and key_count readable C-string pointers until return.
#[unsafe(no_mangle)]
pub(super) extern "C" fn bonsai_qt_bpe_load_specials(
    owner: *mut c_void,
    path: *const c_char,
    keys: *const *const c_char,
    key_count: c_int,
) -> c_int {
    guard(|| {
        if path.is_null() {
            return -1;
        }
        let Some(owner) = owner_mut(owner) else {
            return -1;
        };
        let count = usize::try_from(key_count.max(0)).unwrap_or(usize::MAX);
        if count > 64 || (count != 0 && (keys.is_null() || !keys.is_aligned())) {
            return -1;
        }
        let pointers = if count == 0 {
            &[][..]
        } else {
            // SAFETY: The native caller supplies count live pointer entries,
            // disjoint from the mutably borrowed owner; count is bounded above.
            unsafe { std::slice::from_raw_parts(keys, count) }
        };
        let mut names = Vec::new();
        if names.try_reserve_exact(count).is_err() {
            return -1;
        }
        for &name in pointers {
            if name.is_null() {
                return -1;
            }
            // SAFETY: Each borrowed key pointer names a native C string whose
            // storage remains live through this synchronous setup call.
            names.push(unsafe { CStr::from_ptr(name) });
        }
        // SAFETY: The native path storage remains NUL-terminated and live.
        let path = unsafe { CStr::from_ptr(path) };
        let Ok(metadata) = Gguf::open(path, &NATIVE_API) else {
            return 0;
        };
        if specials(owner, &metadata, names).is_ok() {
            1
        } else {
            -1
        }
    })
    .unwrap_or(-1)
}

#[cfg(test)]
#[path = "tokenizer_loading_tests.rs"]
mod tests;
