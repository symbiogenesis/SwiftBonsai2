//! Model-free encoder for the pinned public QAIC start/stop wire control.
//!
//! The source prediction uses QAIC a66a0019fb488abae955bbc9b3a9de3dc15962d0,
//! public FastRPC imports a56e9d4de3614a8d8f4b0e15ff561350c4b686e3 and HTP IDL
//! 19cce6919992fdffd92e0ac152caaf1e2b8f254e. See `public-qaic.lock.json` and
//! `docs/hexagon-signed-q2-candidate.md` for their distinct provenance boundaries.
//!
//! This only encodes public-control bytes. It has no FFI, driver, SDK, queue,
//! process-init or runtime integration and proves no signed-module compatibility.
//! The generated C mock must be compared independently against this result.
#![forbid(unsafe_code)]
#![deny(warnings)]

use std::io::{self, Write as _};
use std::process::ExitCode;

/// Classic FastRPC scalars: method 2, one input buffer, no outputs or handles.
pub const START_SCALARS: u32 = 0x0201_0000;
/// Classic FastRPC scalars: method 3, no buffers or handles.
pub const STOP_SCALARS: u32 = 0x0300_0000;
/// Complete primary input buffer, including both zero-initialized padding ranges.
pub const START_PRIMARY_BYTES: usize = 48;

/// Typed arguments of the pinned public seven-argument HTP start declaration.
///
/// The remote handle is passed separately to invoke and is not an eighth field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StartFields {
    /// Session identifier, copied at byte offset 0.
    pub sess_id: u32,
    /// Queue identifier, copied at byte offset 8 after four zero bytes.
    pub dsp_queue_id: u64,
    /// Requested HVX count, copied at byte offset 16.
    pub n_hvx: u32,
    /// Requested HMX count, copied at byte offset 20.
    pub n_hmx: u32,
    /// Requested VTCM size, copied at byte offset 24.
    pub vtcm_size: u64,
    /// Mapping window argument, copied at byte offset 32.
    pub max_vmem: u64,
    /// Power-mode argument, copied at byte offset 40 before four zero tail bytes.
    pub power_mode: u32,
}

impl StartFields {
    /// Encode the public-control layout explicitly, independent of Rust/C layout.
    ///
    /// `Serialize.alignAdd` derives offsets 0/8/16/20/24/32/40. The logical
    /// extent is 44 bytes with alignment 8; `ToCStub.declarePrim` chooses six
    /// zero-initialized u64 elements and publishes their complete 48-byte size.
    /// Explicit little endian corresponds to the hosted generated-C control's
    /// little-endian target; this is not a universal vendor wire-format claim.
    #[must_use]
    pub const fn encode(self) -> [u8; START_PRIMARY_BYTES] {
        let [s0, s1, s2, s3] = self.sess_id.to_le_bytes();
        let [q0, q1, q2, q3, q4, q5, q6, q7] = self.dsp_queue_id.to_le_bytes();
        let [v0, v1, v2, v3] = self.n_hvx.to_le_bytes();
        let [h0, h1, h2, h3] = self.n_hmx.to_le_bytes();
        let [t0, t1, t2, t3, t4, t5, t6, t7] = self.vtcm_size.to_le_bytes();
        let [m0, m1, m2, m3, m4, m5, m6, m7] = self.max_vmem.to_le_bytes();
        let [p0, p1, p2, p3] = self.power_mode.to_le_bytes();
        [
            s0, s1, s2, s3, 0, 0, 0, 0, q0, q1, q2, q3, q4, q5, q6, q7, v0, v1, v2, v3, h0, h1, h2,
            h3, t0, t1, t2, t3, t4, t5, t6, t7, m0, m1, m2, m3, m4, m5, m6, m7, p0, p1, p2, p3, 0,
            0, 0, 0,
        ]
    }
}

const TEST_VECTOR_ID: &str = "htp-start-distinct-positive-v1";
const SYNTHETIC_HANDLE: u64 = 0x1020_3040_5060_7080;
const SYNTHETIC_START: StartFields = StartFields {
    sess_id: 0x1122_3344,
    dsp_queue_id: 0x2132_4354_6576_0718,
    n_hvx: 0x3142_5364,
    n_hmx: 0x4152_6374,
    vtcm_size: 0x5162_7304_1526_3748,
    max_vmem: 0x6172_0314_2536_4758,
    power_mode: 0x7182_0314,
};

fn control_json() -> String {
    let primary = SYNTHETIC_START.encode();
    let mut primary_hex = String::with_capacity(START_PRIMARY_BYTES * 2);
    for byte in primary {
        primary_hex.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        primary_hex.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    format!(
        "{{\"schemaVersion\":1,\"kind\":\"public-idl-rust-encoding-control\",\"testVectorId\":\"{TEST_VECTOR_ID}\",\"handleHex\":\"{SYNTHETIC_HANDLE:016x}\",\"start\":{{\"scalars\":{START_SCALARS},\"method\":2,\"inputBuffers\":1,\"outputBuffers\":0,\"inputHandles\":0,\"outputHandles\":0,\"argsNull\":false,\"primaryBytes\":{START_PRIMARY_BYTES},\"primaryHex\":\"{primary_hex}\"}},\"stop\":{{\"scalars\":{STOP_SCALARS},\"method\":3,\"inputBuffers\":0,\"outputBuffers\":0,\"inputHandles\":0,\"outputHandles\":0,\"argsNull\":true,\"primaryBytes\":0,\"primaryHex\":\"\"}},\"driverLoaded\":false,\"deviceExecuted\":false,\"sdkCompatibilityQualified\":false,\"profileEligible\":false}}\n"
    )
}

fn main() -> ExitCode {
    if std::env::args_os().len() != 1 {
        let _ = writeln!(
            io::stderr().lock(),
            "The public IDL control accepts no arguments."
        );
        return ExitCode::FAILURE;
    }
    if io::stdout()
        .lock()
        .write_all(control_json().as_bytes())
        .is_err()
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_independently_derived_public_start_stop_vector() {
        // These bytes were hand-derived from the pinned generator before the
        // Rust encoder existed and before observing generated stub execution.
        // The separately built generated-C mock captures its actual input and
        // is compared by the owning hosted control, rather than recreated here.
        let expected = [
            0x44, 0x33, 0x22, 0x11, 0x00, 0x00, 0x00, 0x00, 0x18, 0x07, 0x76, 0x65, 0x54, 0x43,
            0x32, 0x21, 0x64, 0x53, 0x42, 0x31, 0x74, 0x63, 0x52, 0x41, 0x48, 0x37, 0x26, 0x15,
            0x04, 0x73, 0x62, 0x51, 0x58, 0x47, 0x36, 0x25, 0x14, 0x03, 0x72, 0x61, 0x14, 0x03,
            0x82, 0x71, 0x00, 0x00, 0x00, 0x00,
        ];
        assert_eq!(SYNTHETIC_START.encode(), expected);
        assert_eq!(START_SCALARS, 33_619_968);
        assert_eq!(STOP_SCALARS, 50_331_648);
    }
}
