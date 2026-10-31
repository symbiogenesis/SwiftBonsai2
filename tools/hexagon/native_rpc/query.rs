//! Bounded-supervisor child for the native Rust FastRPC capability candidate.
#![forbid(unsafe_code)]
#![deny(warnings)]

use hexagon_native_rpc::{CAPABILITIES, Capability, Driver, DriverLocation, decode_architecture};
use std::io::{self, Write as _};
use std::process::ExitCode;

fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", u32::from(value));
            }
            value => output.push(value),
        }
    }
    output.push('"');
    output
}

fn run() -> Result<String, hexagon_native_rpc::Error> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 1 {
        return Err(hexagon_native_rpc::Error::Contract);
    }
    if arguments[0] == "--self-test" {
        hexagon_native_rpc::self_check()?;
        return Ok(String::from(
            "{\"schemaVersion\":1,\"kind\":\"native-fastrpc-self-check\",\"passed\":true,\"driverLoaded\":false,\"dspCodeLoaded\":false,\"profileEligible\":false}",
        ));
    }
    if arguments[0] == "--inspect" {
        let location = DriverLocation::inspect_system()?;
        return Ok(format!(
            "{{\"schemaVersion\":1,\"kind\":\"native-fastrpc-inspection\",\"driverDll\":{},\"driverLoaded\":false,\"dspCodeLoaded\":false,\"profileEligible\":false}}",
            quote(&location.path().to_string_lossy())
        ));
    }
    let forbidden = [
        "ADSP_", "CDSP_", "FASTRPC_", "RPCMEM_", "GGML_", "GENIEX_", "QNN_", "QAIRT_", "HEXAGON_",
    ];
    if std::env::vars_os().any(|(name, _)| {
        let name = name.to_string_lossy().to_ascii_uppercase();
        forbidden.iter().any(|prefix| name.starts_with(prefix))
    }) {
        return Err(hexagon_native_rpc::Error::InheritedOverride);
    }
    if arguments[0] == "--mapping-probe" {
        let driver = Driver::open_system()?;
        let driver_path = driver.path().to_string_lossy().into_owned();
        let report = driver.scratch_mapping_probe()?;
        let raw = |value: Option<i32>| {
            value.map_or_else(|| String::from("null"), |value| value.to_string())
        };
        return Ok(format!(
            "{{\"schemaVersion\":1,\"kind\":\"native-fastrpc-mapping-probe\",\"driverDll\":{},\"passed\":{},\"stage\":{},\"bytes\":{},\"domain\":3,\"heapId\":25,\"allocationFlags\":1,\"mapFlags\":0,\"allocationSucceeded\":{},\"descriptorObserved\":{},\"descriptorError\":{},\"cpuRoundtripVerified\":{},\"mappingAttempted\":{},\"mappingResult\":{},\"unmappingResult\":{},\"freeCompleted\":{},\"poisoned\":{},\"ownerPinnedUntilExit\":{},\"implicitSessionMayOpen\":true,\"customDspModuleLoaded\":false,\"dmaExecuted\":false,\"computeExecuted\":false,\"sessionTeardownVerified\":false,\"profileEligible\":false}}",
            quote(&driver_path),
            report.completed_mapping_cycle(),
            quote(report.stage),
            report.bytes,
            report.allocation_succeeded,
            report.descriptor_observed,
            raw(report.descriptor_error),
            report.cpu_roundtrip_verified,
            report.mapping_attempted,
            raw(report.mapping_result),
            raw(report.unmapping_result),
            report.free_completed,
            report.poisoned,
            report.owner_pinned_until_exit
        ));
    }
    if arguments[0] != "--query" {
        return Err(hexagon_native_rpc::Error::Contract);
    }
    let mut driver = Driver::open_system()?;
    let driver_path = driver.path().to_string_lossy().into_owned();
    let mut queries = Vec::with_capacity(CAPABILITIES.len());
    for capability in CAPABILITIES {
        let observation = match driver.capability(capability) {
            Ok(raw) => {
                let arch = if capability == Capability::Architecture {
                    decode_architecture(raw)
                        .map_or_else(|| String::from("null"), |value| value.to_string())
                } else {
                    String::from("null")
                };
                format!(
                    "{{\"attribute\":{},\"status\":\"queried\",\"raw\":{raw},\"decodedArchitecture\":{arch}}}",
                    quote(capability.name())
                )
            }
            Err(error) => format!(
                "{{\"attribute\":{},\"status\":\"unavailable\",\"raw\":null,\"error\":{}}}",
                quote(capability.name()),
                quote(&error.to_string())
            ),
        };
        queries.push(observation);
    }
    drop(driver);
    Ok(format!(
        "{{\"schemaVersion\":1,\"kind\":\"native-fastrpc-capabilities\",\"driverDll\":{},\"domain\":3,\"requestId\":2,\"queries\":[{}],\"signaturePolicy\":\"cached-authenticode-no-ui\",\"driverLoaded\":true,\"dspCodeLoaded\":false,\"mappingExecuted\":false,\"inferenceExecuted\":false,\"profileEligible\":false,\"ggmlSource\":{},\"fastrpcSource\":{}}}",
        quote(&driver_path),
        queries.join(","),
        quote(hexagon_native_rpc::GGML_SOURCE),
        quote(hexagon_native_rpc::FASTRPC_SOURCE)
    ))
}

fn main() -> ExitCode {
    let (message, result) = match run() {
        Ok(message) => (message, ExitCode::SUCCESS),
        Err(error) => (
            format!(
                "{{\"schemaVersion\":1,\"kind\":\"native-fastrpc-capabilities\",\"status\":\"pending\",\"error\":{},\"profileEligible\":false}}",
                quote(&error.to_string())
            ),
            ExitCode::FAILURE,
        ),
    };
    if writeln!(io::stdout().lock(), "{message}").is_err() {
        return ExitCode::FAILURE;
    }
    result
}
