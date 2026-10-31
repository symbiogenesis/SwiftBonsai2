//! The production GGUF adapter and loading path, with bounded ABI fault injection.

use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char, c_int, c_void},
    io,
};

use super::super::{
    OWNER_COUNTS, REJECT, RESULT_COUNTS, bonsai_qt_bpe_add_special, bonsai_qt_bpe_encode_to,
    bonsai_qt_bpe_free, encode_copy_for_test, encode_to,
};
use super::{
    ARRAY, Api, Gguf, InitParams, STRING, UINT32, bonsai_qt_bpe_load_gguf,
    bonsai_qt_bpe_load_specials, owner_mut, reload, specials,
};

#[derive(Clone)]
struct Fixture {
    tokens: Vec<Option<CString>>,
    merges: Vec<Option<CString>>,
    tokens_present: bool,
    merges_present: bool,
    oversized: bool,
    array_type: c_int,
    element_type: c_int,
    special_type: c_int,
}

impl Default for Fixture {
    fn default() -> Self {
        Self {
            tokens: [c"a", c"a", c"<eos>"]
                .map(|text| Some(text.to_owned()))
                .to_vec(),
            merges: [c"a a", c"a a"].map(|text| Some(text.to_owned())).to_vec(),
            tokens_present: true,
            merges_present: true,
            oversized: false,
            array_type: ARRAY,
            element_type: STRING,
            special_type: UINT32,
        }
    }
}

std::thread_local! {
    static FIXTURE: RefCell<Fixture> = RefCell::new(Fixture::default());
    static GGUF_COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    static READS: RefCell<Vec<(i64, usize)>> = const { RefCell::new(Vec::new()) };
    pub(super) static SPECIAL_IDS: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    static PARAMS_CORRECT: Cell<bool> = const { Cell::new(false) };
}

// SAFETY: All test API pointers originate from Box<Fixture> in open and remain
// live while the production Rust GGUF guard borrows them. No mutation overlaps.
fn fixture<'a>(context: *const c_void) -> &'a Fixture {
    // SAFETY: The private test API enforces the allocation/lifetime above.
    unsafe { &*context.cast::<Fixture>() }
}

unsafe extern "C-unwind" fn open(path: *const c_char, params: InitParams) -> *mut c_void {
    PARAMS_CORRECT.with(|correct| correct.set(params.no_alloc && params.context.is_null()));
    // SAFETY: The production adapter supplies a live NUL-terminated CStr path.
    if unsafe { CStr::from_ptr(path) } != c"fixture" {
        return std::ptr::null_mut();
    }
    GGUF_COUNTS.with(|counts| {
        let (opened, closed) = counts.get();
        counts.set((opened + 1, closed));
    });
    FIXTURE.with(|value| Box::into_raw(Box::new(value.borrow().clone())).cast())
}

unsafe extern "C-unwind" fn close(context: *mut c_void) {
    // SAFETY: The production guard returns its one original Box pointer once,
    // after all string borrows and before the API function table goes away.
    drop(unsafe { Box::from_raw(context.cast::<Fixture>()) });
    GGUF_COUNTS.with(|counts| {
        let (opened, closed) = counts.get();
        counts.set((opened, closed + 1));
    });
}

unsafe extern "C-unwind" fn find(context: *const c_void, name: *const c_char) -> i64 {
    let fixture = fixture(context);
    // SAFETY: The adapter supplies a live borrowed CStr for each lookup.
    let name = unsafe { CStr::from_ptr(name) };
    if name == super::TOKENS && fixture.tokens_present {
        0
    } else if name == super::MERGES && fixture.merges_present {
        1
    } else if name == c"first" {
        2
    } else if name == c"invalid" {
        3
    } else {
        -1
    }
}

unsafe extern "C-unwind" fn kind(context: *const c_void, key: i64) -> c_int {
    let fixture = fixture(context);
    if key < 2 {
        fixture.array_type
    } else {
        fixture.special_type
    }
}

unsafe extern "C-unwind" fn array_kind(context: *const c_void, _: i64) -> c_int {
    fixture(context).element_type
}

unsafe extern "C-unwind" fn count(context: *const c_void, key: i64) -> usize {
    let fixture = fixture(context);
    if fixture.oversized {
        return usize::try_from(1_u64 << 32).unwrap_or(usize::MAX);
    }
    if key == 0 {
        fixture.tokens.len()
    } else {
        fixture.merges.len()
    }
}

unsafe extern "C-unwind" fn string(
    context: *const c_void,
    key: i64,
    index: usize,
) -> *const c_char {
    READS.with(|reads| reads.borrow_mut().push((key, index)));
    let fixture = fixture(context);
    let values = if key == 0 {
        &fixture.tokens
    } else {
        &fixture.merges
    };
    values
        .get(index)
        .and_then(Option::as_ref)
        .map_or(std::ptr::null(), |value| value.as_ptr())
}

unsafe extern "C-unwind" fn u32_value(_: *const c_void, key: i64) -> u32 {
    if key == 2 { 1 } else { u32::MAX }
}

const API: Api = Api {
    open,
    close,
    find,
    kind,
    array_kind,
    count,
    string,
    u32_value,
};

struct Slot(*mut c_void);

impl Slot {
    const fn new() -> Self {
        Self(std::ptr::null_mut())
    }

    fn load(&mut self, path: &CStr) -> io::Result<()> {
        reload(&mut self.0, path, &API)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        bonsai_qt_bpe_free(self.0);
    }
}

struct Fault;

impl Fault {
    fn at(stage: u8) -> Self {
        REJECT.with(|reject| reject.set(stage));
        Self
    }
}

impl Drop for Fault {
    fn drop(&mut self) {
        REJECT.with(|reject| reject.set(0));
    }
}

fn balanced_metadata() {
    GGUF_COUNTS.with(|counts| {
        let (opened, closed) = counts.get();
        assert_eq!(opened, closed);
    });
}

fn reset() {
    OWNER_COUNTS.with(|counts| counts.set((0, 0)));
    RESULT_COUNTS.with(|counts| counts.set((0, 0)));
    GGUF_COUNTS.with(|counts| counts.set((0, 0)));
    REJECT.with(|reject| reject.set(0));
    FIXTURE.with(|value| *value.borrow_mut() = Fixture::default());
    READS.with(|reads| reads.borrow_mut().clear());
    SPECIAL_IDS.with(|ids| ids.borrow_mut().clear());
}

#[test]
fn rust_gguf_loader_owns_metadata_candidates_and_transactional_replacement() -> io::Result<()> {
    reset();
    let mut slot = Slot::new();
    assert!(slot.load(c"missing").is_err());
    assert!(slot.0.is_null());
    assert_eq!(OWNER_COUNTS.with(Cell::get), (0, 0));
    slot.load(c"fixture")?;
    assert!(PARAMS_CORRECT.with(Cell::get));
    let original = slot.0;
    assert_eq!(OWNER_COUNTS.with(Cell::get), (1, 0));
    assert_eq!(
        READS.with(|reads| reads.borrow().clone()),
        [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1)]
    );
    let table = owner_mut(slot.0).ok_or_else(super::invalid)?;
    assert_eq!(table.token_id_for_test(b"a"), Some(1));
    assert_eq!(table.token_id_for_test(b"<eos>"), Some(2));
    assert_eq!(table.merge_rank_for_test(b"a a"), Some(1));
    // Repeated vocabulary spellings retain the last original array index.
    assert_eq!(encode_copy_for_test(slot.0, b"a", false), Some(vec![1]));
    balanced_metadata();
    for stage in 1..=4 {
        let before = OWNER_COUNTS.with(Cell::get);
        let fault = Fault::at(stage);
        assert!(slot.load(c"fixture").is_err());
        assert_eq!(slot.0, original);
        let after = OWNER_COUNTS.with(Cell::get);
        let created = usize::from(stage != 1);
        assert_eq!(after, (before.0 + created, before.1 + created));
        balanced_metadata();
        drop(fault);
        assert_eq!(encode_copy_for_test(slot.0, b"a", false), Some(vec![1]));
    }
    for mode in 0..7 {
        FIXTURE.with(|value| {
            let mut value = value.borrow_mut();
            match mode {
                0 => value.tokens_present = false,
                1 => value.merges_present = false,
                2 => value.tokens[0] = None,
                3 => value.merges[0] = None,
                4 => value.oversized = true,
                5 => value.array_type = UINT32,
                _ => value.element_type = UINT32,
            }
        });
        assert!(slot.load(c"fixture").is_err());
        assert_eq!(slot.0, original);
        balanced_metadata();
        FIXTURE.with(|value| *value.borrow_mut() = Fixture::default());
    }
    let before = OWNER_COUNTS.with(Cell::get);
    slot.load(c"fixture")?;
    assert_ne!(slot.0, original);
    assert_eq!(OWNER_COUNTS.with(Cell::get), (before.0 + 1, before.1 + 1));
    assert_eq!(encode_copy_for_test(slot.0, b"a", false), Some(vec![1]));
    balanced_metadata();
    drop(slot);
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}

#[test]
fn rust_special_intake_preserves_order_signed_ids_errors_and_cleanup() -> io::Result<()> {
    reset();
    let mut slot = Slot::new();
    slot.load(c"fixture")?;
    {
        let metadata = Gguf::open(c"fixture", &API)?;
        let owner = owner_mut(slot.0).ok_or_else(super::invalid)?;
        specials(
            owner,
            &metadata,
            [c"missing", c"first", c"invalid", c"first"],
        )?;
        assert_eq!(SPECIAL_IDS.with(|ids| ids.borrow().clone()), [1, -1, 1]);
    }
    balanced_metadata();
    assert!(bonsai_qt_bpe_add_special(slot.0, b"<voice>".as_ptr(), 7, 2));
    assert_eq!(
        encode_copy_for_test(slot.0, b"<voice>", false),
        Some(vec![2])
    );
    {
        let _fault = Fault::at(5);
        assert!(!bonsai_qt_bpe_add_special(slot.0, b"bad".as_ptr(), 3, 0));
    }
    {
        let _fault = Fault::at(6);
        let metadata = Gguf::open(c"fixture", &API)?;
        let owner = owner_mut(slot.0).ok_or_else(super::invalid)?;
        assert!(specials(owner, &metadata, [c"first"]).is_err());
    }
    balanced_metadata();
    FIXTURE.with(|value| value.borrow_mut().special_type = STRING);
    {
        let metadata = Gguf::open(c"fixture", &API)?;
        let owner = owner_mut(slot.0).ok_or_else(super::invalid)?;
        assert!(specials(owner, &metadata, [c"first"]).is_err());
    }
    balanced_metadata();
    Ok(())
}

#[test]
#[allow(
    clippy::panic,
    reason = "Deliberate safe-Rust unwind verifies owned encoding cleanup before the ABI guard returns failure."
)]
fn rust_encoding_owns_results_through_copy_failure_empty_output_and_unwind() -> io::Result<()> {
    reset();
    FIXTURE.with(|value| {
        value.borrow_mut().tokens = [c"a", c"b", c"\u{100}", c"<|endoftext|>"]
            .map(|text| Some(text.to_owned()))
            .to_vec();
        value.borrow_mut().merges.clear();
    });
    let mut slot = Slot::new();
    slot.load(c"fixture")?;
    // The production callback sees text after embedded NULs and exact EOS mode.
    assert_eq!(
        encode_copy_for_test(slot.0, b"a\0b", false),
        Some(vec![0, 2, 1])
    );
    assert_eq!(encode_copy_for_test(slot.0, b"", false), Some(vec![]));
    assert_eq!(encode_copy_for_test(slot.0, b"", true), Some(vec![3]));
    let owner = owner_mut(slot.0).ok_or_else(super::invalid)?;
    assert!(!encode_to(owner, b"a", false, |_| false)?);
    assert!(
        super::super::guard(|| {
            // Deliberately unwind inside safe Rust; the result must drop before the
            // private ABI guard translates the panic to ordinary failure.
            let _ = encode_to(owner, b"a", false, |_| panic!("injected copy failure"));
        })
        .is_none()
    );
    assert!(encode_to(owner, &[0xff], false, |_| true).is_err());
    RESULT_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    assert_eq!(encode_copy_for_test(slot.0, b"ab", false), Some(vec![0, 1]));
    assert!(!bonsai_qt_bpe_encode_to(
        slot.0,
        b"a".as_ptr(),
        1,
        false,
        std::ptr::null_mut(),
        None
    ));
    RESULT_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}

fn gguf_string(output: &mut Vec<u8>, bytes: &[u8]) -> io::Result<()> {
    output.extend(
        u64::try_from(bytes.len())
            .map_err(io::Error::other)?
            .to_le_bytes(),
    );
    output.extend(bytes);
    Ok(())
}

fn tiny_gguf(tokens: &[&[u8]], merges: &[&[u8]]) -> io::Result<Vec<u8>> {
    let mut bytes = b"GGUF".to_vec();
    bytes.extend(3_u32.to_le_bytes());
    bytes.extend(0_u64.to_le_bytes()); // no tensors, no weights or model inference
    bytes.extend(4_u64.to_le_bytes());
    for (name, values) in [(super::TOKENS, tokens), (super::MERGES, merges)] {
        gguf_string(&mut bytes, name.to_bytes())?;
        bytes.extend(9_u32.to_le_bytes());
        bytes.extend(8_u32.to_le_bytes());
        bytes.extend(
            u64::try_from(values.len())
                .map_err(io::Error::other)?
                .to_le_bytes(),
        );
        for value in values {
            gguf_string(&mut bytes, value)?;
        }
    }
    for (name, id) in [
        (b"first".as_slice(), 0_u32),
        (b"invalid".as_slice(), u32::MAX),
    ] {
        gguf_string(&mut bytes, name)?;
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    while bytes.len() % 32 != 0 {
        bytes.push(0);
    }
    Ok(bytes)
}

#[test]
fn real_pinned_gguf_reader_reloads_rust_owned_tables_without_model_weights() -> io::Result<()> {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct File(std::path::PathBuf);
    impl Drop for File {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    reset();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "Bonsai-RustTokenizer-{}-{stamp}.gguf",
        std::process::id()
    ));
    let _file = File(path.clone());
    let path_text = path.to_str().ok_or_else(super::invalid)?;
    let c_path = CString::new(path_text).map_err(io::Error::other)?;
    fs::write(
        &path,
        tiny_gguf(&[b"a", b"a", b"<|endoftext|>"], &[b"a a", b"a a"])?,
    )?;
    let mut slot = Slot::new();
    assert!(bonsai_qt_bpe_load_gguf(&raw mut slot.0, c_path.as_ptr()));
    assert_eq!(encode_copy_for_test(slot.0, b"a", true), Some(vec![1, 2]));
    let original = slot.0;
    let keys = [
        c"missing".as_ptr(),
        c"first".as_ptr(),
        c"invalid".as_ptr(),
        c"first".as_ptr(),
    ];
    assert_eq!(
        bonsai_qt_bpe_load_specials(slot.0, c_path.as_ptr(), keys.as_ptr(), 4),
        1
    );
    assert_eq!(SPECIAL_IDS.with(|ids| ids.borrow().clone()), [0, -1, 0]);
    fs::write(&path, b"invalid GGUF")?;
    assert!(!bonsai_qt_bpe_load_gguf(&raw mut slot.0, c_path.as_ptr()));
    assert_eq!(slot.0, original);
    assert_eq!(encode_copy_for_test(slot.0, b"a", true), Some(vec![1, 2]));
    fs::write(&path, tiny_gguf(&[b"b"], &[])?)?;
    assert!(bonsai_qt_bpe_load_gguf(&raw mut slot.0, c_path.as_ptr()));
    assert_ne!(slot.0, original);
    assert_eq!(encode_copy_for_test(slot.0, b"b", false), Some(vec![0]));
    drop(slot);
    OWNER_COUNTS.with(|counts| assert_eq!(counts.get().0, counts.get().1));
    Ok(())
}
