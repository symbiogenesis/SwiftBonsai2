//! Model-free tokenizer replay of immutable independently captured native observations.
//!
//! The complete original input schedule and native metadata/chunk/ID bytes are
//! retained in `tests/fixtures/tokenizer-v1.*`; provenance binds source and target.
//! Rust loading and result lifetime regressions exercise the production GGUF adapter.
#![forbid(unsafe_code)]

use std::{error::Error, ffi::c_void, io};

use sha2::{Digest, Sha256};

use super::tokenizer::{
    bonsai_qt_bpe_add_merge, bonsai_qt_bpe_add_special, bonsai_qt_bpe_add_special_id,
    bonsai_qt_bpe_add_token, bonsai_qt_bpe_encode, bonsai_qt_bpe_finish, bonsai_qt_bpe_free,
    bonsai_qt_bpe_new, bonsai_qt_bpe_result_data, bonsai_qt_bpe_result_free,
    bonsai_qt_bpe_result_len, copy_result_for_test, encode_copy_for_test,
};
use crate::tokenizer::{Tokenizer, pretokenize};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CAPTURED_INPUTS: &[u8] = include_bytes!("../../tests/fixtures/tokenizer-v1.inputs.bin");
const CAPTURED_OUTPUTS: &[u8] = include_bytes!("../../tests/fixtures/tokenizer-v1.expected.bin");
const INPUT_SHA256: [u8; 32] = [
    0x1b, 0x09, 0x0f, 0x66, 0x12, 0x8d, 0xb8, 0x23, 0x27, 0xd8, 0xcf, 0x40, 0x2b, 0x8c, 0x73, 0x07,
    0x8d, 0x83, 0x7e, 0x5d, 0xea, 0x8e, 0x22, 0xc6, 0xc1, 0x22, 0x56, 0x36, 0x11, 0x39, 0x2e, 0x95,
];
const OUTPUT_SHA256: [u8; 32] = [
    0xe9, 0x55, 0xba, 0x85, 0xea, 0xb3, 0x41, 0xef, 0x13, 0x57, 0x4b, 0x98, 0x3b, 0xce, 0xc8, 0xc7,
    0xaa, 0x79, 0xf1, 0xc7, 0xfc, 0xa5, 0x2d, 0x8a, 0x24, 0xfa, 0xa7, 0x04, 0x7d, 0x3a, 0x10, 0xe8,
];

struct Fixture {
    tokens: Vec<Vec<u8>>,
    merges: Vec<Vec<u8>>,
    keys: Vec<(String, u32)>,
    requested_keys: Vec<String>,
    specials: Vec<(Vec<u8>, i32)>,
    requests: Vec<(Vec<u8>, bool)>,
}

struct NativeTokenizer(*mut c_void);

impl NativeTokenizer {
    fn new(fixture: &Fixture) -> Result<Self> {
        let owner = Self(bonsai_qt_bpe_new());
        assert!(!owner.0.is_null());
        for (id, token) in fixture.tokens.iter().enumerate() {
            assert!(bonsai_qt_bpe_add_token(
                owner.0,
                token.as_ptr(),
                token.len(),
                i32::try_from(id)?,
            ));
        }
        for (rank, merge) in fixture.merges.iter().enumerate() {
            assert!(bonsai_qt_bpe_add_merge(
                owner.0,
                merge.as_ptr(),
                merge.len(),
                i32::try_from(rank)?,
            ));
        }
        assert!(bonsai_qt_bpe_finish(owner.0));
        for key in &fixture.requested_keys {
            if let Some((_, id)) = fixture.keys.iter().find(|(name, _)| name == key) {
                assert!(bonsai_qt_bpe_add_special_id(
                    owner.0,
                    i32::from_le_bytes(id.to_le_bytes()),
                ));
            }
        }
        for (token, id) in &fixture.specials {
            assert!(bonsai_qt_bpe_add_special(
                owner.0,
                token.as_ptr(),
                token.len(),
                *id
            ));
        }
        Ok(owner)
    }

    fn encode(&self, text: &[u8], eos: bool) -> Result<Vec<i32>> {
        encode_copy_for_test(self.0, text, eos).ok_or_else(|| {
            io::Error::other("Native Rust tokenizer rejected a reference case.").into()
        })
    }
}

impl Drop for NativeTokenizer {
    fn drop(&mut self) {
        bonsai_qt_bpe_free(self.0);
    }
}

struct NativeResult(*mut c_void);

impl Drop for NativeResult {
    fn drop(&mut self) {
        bonsai_qt_bpe_result_free(self.0);
    }
}

fn safe_tokenizer(fixture: &Fixture) -> Result<Tokenizer> {
    let mut tokenizer = Tokenizer::new();
    for (id, token) in fixture.tokens.iter().enumerate() {
        tokenizer.add_token(token, i32::try_from(id)?)?;
    }
    for (rank, merge) in fixture.merges.iter().enumerate() {
        tokenizer.add_merge(merge, i32::try_from(rank)?)?;
    }
    tokenizer.finish()?;
    for key in &fixture.requested_keys {
        if let Some((_, id)) = fixture.keys.iter().find(|(name, _)| name == key) {
            tokenizer.add_special_id(i32::from_le_bytes(id.to_le_bytes()))?;
        }
    }
    for (token, id) in &fixture.specials {
        tokenizer.add_special(token, *id)?;
    }
    Ok(tokenizer)
}

fn byte_vocabulary() -> Result<Vec<Vec<u8>>> {
    let mut next = 256_u32;
    (u8::MIN..=u8::MAX)
        .map(|byte| {
            let scalar = if (b'!'..=b'~').contains(&byte)
                || (0xa1..=0xac).contains(&byte)
                || (0xae..=0xff).contains(&byte)
            {
                u32::from(byte)
            } else {
                let value = next;
                next += 1;
                value
            };
            char::from_u32(scalar)
                .map(|value| value.to_string().into_bytes())
                .ok_or_else(|| "invalid fixture byte alphabet".into())
        })
        .collect()
}

fn corpus() -> Vec<(Vec<u8>, bool)> {
    let mut requests = Vec::new();
    for text in [
        "",
        "a",
        "ab",
        "abc",
        "aaaa",
        "aaaaa",
        "abab abcab",
        "1234567890",
        "  alpha",
        " \talpha",
        " \t\r\n",
        " !?\r\n",
        "end   ",
        "\0a\0",
        "'S 'T 'RE 'VE 'M 'LL 'D",
        "'still 'twill 'replay",
        "’S ’T ’RE ’VE ’M ’LL ’D",
        "'sα ’sα 's1 ’s!",
        "e\u{301} café Straße",
        "中文 日本語 한국어",
        "😀🦀𐐀",
        "a\u{a0}b",
        "a\u{2000}b",
        "a\u{2001}\u{2002}b",
        " \u{200b}word",
        "\u{2000}\u{2000}word",
        "\u{200b}a\u{200b}b",
        "\u{2028}\u{2029}",
        "<|endoftext|>",
        "before<|endoftext|>after",
        "<x><y><x>",
        "<x><y>",
        "a<x><y>b",
        "<tag>voice</tag>",
        "a<missing>b",
        "<x><x>",
    ] {
        for eos in [false, true] {
            requests.push((text.as_bytes().to_vec(), eos));
        }
    }
    // Pinned scanner category boundaries differ from Rust Unicode properties.
    for scalar in [
        0xbf,
        0xc0,
        0xd6,
        0xd7,
        0xd8,
        0xf6,
        0xf7,
        0xf8,
        0x24f,
        0x250,
        0x36f,
        0x370,
        0x1fff,
        0x2000,
        0x2003,
        0x2bff,
        0x2c00,
        0x2dff,
        0x2e00,
        0x303f,
        0x3040,
        0x9fff,
        0xa000,
        0xabff,
        0xac00,
        0xd7af,
        0xd7b0,
        0xf8ff,
        0xf900,
        0xfaff,
        0xfb00,
        0xffff,
        0x0001_0000,
        0x0010_ffff,
    ] {
        if let Some(character) = char::from_u32(scalar) {
            requests.push((format!("A{character}b {character}3\n").into_bytes(), false));
        }
    }
    let alphabet = [
        'a', 'b', 'c', 'x', 'y', '0', '9', ' ', '\t', '\n', '\r', '\'', '’', '!', '.', 'é', 'α',
        '中', '\u{301}', '\u{a0}', '\u{2000}', '\u{200b}', '😀',
    ];
    let mut state = 0x1a2b_3c4d_u32;
    for index in 0..256 {
        let mut text = String::new();
        for _ in 0..index % 47 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let slot = usize::from(state.to_le_bytes()[0]) % alphabet.len();
            text.push(alphabet[slot]);
        }
        requests.push((text.into_bytes(), index % 2 == 0));
    }
    requests
}

fn fixtures() -> Result<Vec<Fixture>> {
    let mut tokens = byte_vocabulary()?;
    tokens.extend(
        [
            "ab",
            "abc",
            "aa",
            "aaa",
            "aaaa",
            "Ġa",
            "xy",
            "<|endoftext|>",
            "<x>",
            "<x><y>",
            "<y>",
            "<tag>",
            "</tag>",
        ]
        .map(|text| text.as_bytes().to_vec()),
    );
    let merges = [
        "a b", "ab c", "a a", "aa a", "a aa", "aa aa", "Ġ a", "x y", "a b",
    ]
    .map(|text| text.as_bytes().to_vec())
    .to_vec();
    let fixture = Fixture {
        tokens: tokens.clone(),
        merges: merges.clone(),
        keys: vec![
            ("text.start".into(), 264),
            ("text.long".into(), 265),
            ("text.end".into(), 266),
            ("text.invalid".into(), u32::MAX),
        ],
        requested_keys: [
            "missing",
            "text.start",
            "text.long",
            "text.end",
            "text.invalid",
            "text.start",
        ]
        .map(str::to_owned)
        .to_vec(),
        specials: vec![
            (b"<x>".to_vec(), 266),
            (b"<tag>".to_vec(), 267),
            (b"</tag>".to_vec(), 268),
        ],
        requests: corpus(),
    };
    let reversed = Fixture {
        tokens,
        merges,
        keys: vec![("long".into(), 265), ("short".into(), 264)],
        requested_keys: vec!["long".into(), "short".into()],
        specials: Vec::new(),
        requests: corpus(),
    };
    let mut duplicated = byte_vocabulary()?;
    duplicated.extend([b"a".to_vec(), b"<|endoftext|>".to_vec()]);
    let duplicated = Fixture {
        tokens: duplicated,
        merges: Vec::new(),
        keys: vec![
            ("hole".into(), 97),
            ("last".into(), 256),
            ("outside".into(), 257),
        ],
        requested_keys: vec!["hole".into(), "last".into(), "outside".into()],
        specials: Vec::new(),
        requests: corpus(),
    };
    let fallback = Fixture {
        tokens: vec![
            vec![0xc3],
            vec![0x83],
            vec![0xc2],
            vec![0xa9],
            b"a".to_vec(),
            b"b".to_vec(),
            b"<|endoftext|>".to_vec(),
        ],
        merges: vec![b"a b".to_vec()],
        keys: Vec::new(),
        requested_keys: Vec::new(),
        specials: Vec::new(),
        requests: corpus(),
    };
    let empty = Fixture {
        tokens: Vec::new(),
        merges: Vec::new(),
        keys: Vec::new(),
        requested_keys: Vec::new(),
        specials: Vec::new(),
        requests: corpus(),
    };
    Ok(vec![fixture, reversed, duplicated, fallback, empty])
}

fn write_u32(output: &mut Vec<u8>, value: usize) -> Result<()> {
    output.extend(u32::try_from(value)?.to_le_bytes());
    Ok(())
}

fn write_text(output: &mut Vec<u8>, text: &[u8]) -> Result<()> {
    write_u32(output, text.len())?;
    output.extend(text);
    Ok(())
}

fn fixture_bytes(fixtures: &[Fixture]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    write_u32(&mut output, fixtures.len())?;
    for fixture in fixtures {
        for values in [&fixture.tokens, &fixture.merges] {
            write_u32(&mut output, values.len())?;
            for value in values {
                write_text(&mut output, value)?;
            }
        }
        write_u32(&mut output, fixture.keys.len())?;
        for (key, id) in &fixture.keys {
            write_text(&mut output, key.as_bytes())?;
            output.extend(id.to_le_bytes());
        }
        write_u32(&mut output, fixture.requested_keys.len())?;
        for key in &fixture.requested_keys {
            write_text(&mut output, key.as_bytes())?;
        }
        write_u32(&mut output, fixture.specials.len())?;
        for (text, id) in &fixture.specials {
            write_text(&mut output, text)?;
            output.extend(id.to_le_bytes());
        }
        write_u32(&mut output, fixture.requests.len())?;
        for (text, eos) in &fixture.requests {
            write_text(&mut output, text)?;
            output.extend(u32::from(*eos).to_le_bytes());
        }
    }
    Ok(output)
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn word(&mut self) -> Result<u32> {
        let (word, remaining) = self
            .0
            .split_first_chunk::<4>()
            .ok_or("truncated captured tokenizer word")?;
        self.0 = remaining;
        Ok(u32::from_le_bytes(*word))
    }

    fn text(&mut self) -> Result<&'a [u8]> {
        let size = usize::try_from(self.word()?)?;
        let (value, remaining) = self
            .0
            .split_at_checked(size)
            .ok_or("truncated captured tokenizer text")?;
        self.0 = remaining;
        Ok(value)
    }

    fn words(&mut self) -> Result<Vec<i32>> {
        let count = self.word()?;
        (0..count)
            .map(|_| {
                self.word()
                    .map(|word| i32::from_le_bytes(word.to_le_bytes()))
            })
            .collect()
    }
}

#[test]
fn rust_tokenizer_core_and_abi_match_captured_native_observations() -> Result<()> {
    let fixtures = fixtures()?;
    assert_eq!(fixtures.len(), 5);
    assert!(fixtures.iter().all(|fixture| fixture.requests.len() == 362));
    // Preserve every exact metadata byte, request and EOS bit from the original
    // independent schedule; neither reordering nor a smaller corpus can pass.
    assert_eq!(fixture_bytes(&fixtures)?, CAPTURED_INPUTS);
    let input_digest: [u8; 32] = Sha256::digest(CAPTURED_INPUTS).into();
    let output_digest: [u8; 32] = Sha256::digest(CAPTURED_OUTPUTS).into();
    assert_eq!(input_digest, INPUT_SHA256);
    assert_eq!(output_digest, OUTPUT_SHA256);
    assert_eq!(CAPTURED_INPUTS.len(), 71250);
    assert_eq!(CAPTURED_OUTPUTS.len(), 298_339);
    let mut reader = Reader(CAPTURED_OUTPUTS);
    for (model_index, fixture) in fixtures.iter().enumerate() {
        let safe = safe_tokenizer(fixture)?;
        let native = NativeTokenizer::new(fixture)?;
        // Retain/consume original loader metadata observations without replacing
        // its n_vocab, reverse-hole or special-registration decisions in Rust.
        let _native_vocabulary = reader.word()?;
        let _native_eos = reader.word()?;
        for _ in 0..reader.word()? {
            let _spelling = reader.text()?;
        }
        for _ in 0..reader.word()? {
            let _spelling = reader.text()?;
            let _id = reader.word()?;
        }
        for (request_index, (text, eos)) in fixture.requests.iter().enumerate() {
            let chunks = (0..reader.word()?)
                .map(|_| reader.text())
                .collect::<Result<Vec<_>>>()?;
            assert_eq!(
                pretokenize(text)?,
                chunks,
                "captured scanner mismatch in model {model_index} request {request_index}"
            );
            let expected = reader.words()?;
            assert_eq!(
                safe.encode(text, *eos)?,
                expected,
                "captured safe tokenizer mismatch in model {model_index} request {request_index}"
            );
            assert_eq!(
                native.encode(text, *eos)?,
                expected,
                "captured native ABI mismatch in model {model_index} request {request_index}"
            );
        }
    }
    assert_eq!(reader.0, []);
    Ok(())
}

#[test]
fn malformed_and_oversized_input_reject_without_poisoning_the_tokenizer() -> Result<()> {
    let mut fixtures = fixtures()?;
    let fixture = fixtures.remove(0);
    let safe = safe_tokenizer(&fixture)?;
    let native = NativeTokenizer::new(&fixture)?;
    for bytes in [vec![0xc3], vec![0xff], vec![0xe2, 0x80], vec![b'a'; 65537]] {
        assert!(safe.encode(&bytes, false).is_err());
        assert!(pretokenize(&bytes).is_err());
        assert!(native.encode(&bytes, false).is_err());
    }
    assert_eq!(safe.encode(b"abc", true)?, native.encode(b"abc", true)?);
    Ok(())
}

#[test]
fn tokenizer_setup_and_result_owners_preserve_their_lifetimes() -> Result<()> {
    let mut safe = Tokenizer::new();
    assert!(safe.encode(b"a", false).is_err());
    assert!(safe.add_special(b"", 0).is_err());
    assert!(safe.add_token(b"a", -1).is_err());
    assert!(safe.add_merge(b"a a", -1).is_err());
    assert!(safe.add_token(b"a", 1_000_001).is_err());
    assert!(safe.add_token(&vec![b'a'; 4097], 0).is_err());
    safe.add_token(b"a", 0)?;
    safe.finish()?;
    assert!(safe.finish().is_err());
    assert!(safe.add_token(b"b", 1).is_err());
    assert!(safe.add_merge(b"a a", 0).is_err());
    assert_eq!(safe.encode(b"a", false)?, [0]);

    let owner = NativeTokenizer(bonsai_qt_bpe_new());
    assert!(!owner.0.is_null());
    let unfinished = NativeResult(bonsai_qt_bpe_encode(owner.0, b"a".as_ptr(), 1, false));
    assert!(unfinished.0.is_null());
    assert!(!bonsai_qt_bpe_add_token(owner.0, b"a".as_ptr(), 1, -1));
    assert!(bonsai_qt_bpe_add_token(owner.0, b"a".as_ptr(), 1, 0));
    assert!(bonsai_qt_bpe_finish(owner.0));
    assert!(!bonsai_qt_bpe_finish(owner.0));
    assert!(!bonsai_qt_bpe_add_token(owner.0, b"b".as_ptr(), 1, 1));
    assert!(!bonsai_qt_bpe_add_special(owner.0, std::ptr::null(), 0, 0));
    let empty = NativeResult(bonsai_qt_bpe_encode(owner.0, std::ptr::null(), 0, false));
    assert!(!empty.0.is_null());
    assert_eq!(bonsai_qt_bpe_result_len(empty.0), 0);
    assert!(bonsai_qt_bpe_result_data(empty.0).is_null());
    let result = NativeResult(bonsai_qt_bpe_encode(owner.0, b"a".as_ptr(), 1, false));
    assert!(!result.0.is_null());
    assert_eq!(bonsai_qt_bpe_result_len(result.0), 1);
    assert!(!bonsai_qt_bpe_result_data(result.0).is_null());
    drop(owner);
    assert_eq!(copy_result_for_test(result.0), Some(vec![0]));
    assert_eq!(copy_result_for_test(empty.0), Some(Vec::new()));
    bonsai_qt_bpe_free(std::ptr::null_mut());
    bonsai_qt_bpe_result_free(std::ptr::null_mut());
    Ok(())
}
