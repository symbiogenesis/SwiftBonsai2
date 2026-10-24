//! Safe Rust ports of tokenizer-0.py and tokenizer-0.sh file workflows.
//! Original file arguments remain typed APIs. Acceptance selects the pinned
//! Python source itself as text for all26 declared tokenizer identities; neither
//! original script supplies a default input. Differences remain diagnostic.
use super::engine::Engine;
use super::tokenizer_reference::Reference;
use super::{Result, invalid, result};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write as _};
use std::path::Path;
use std::time::Instant;

fn input(path: &Path) -> Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err(invalid("Tokenizer file input must be absolute"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err(invalid(
            "Tokenizer file input exceeds65536-byte protocol bound",
        ));
    }
    Ok(bytes)
}
fn write_tokens(path: &Path, tokens: &[u32]) -> Result<()> {
    if !path.is_absolute() {
        return Err(invalid("Tokenizer output must be absolute"));
    }
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    for token in tokens {
        write!(output, "{token}\r\n")?;
    }
    output.flush()?;
    Ok(())
}
fn python_text(bytes: &[u8]) -> Result<String> {
    Ok(std::str::from_utf8(bytes)?
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}
fn cpp_text(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == 0x1a {
            break;
        }
        if byte != b'\r' || bytes.get(index + 1) != Some(&b'\n') {
            output.push(byte);
        }
    }
    output
}
pub(super) fn export_reference(
    directory: &Path,
    input_path: &Path,
    output: &Path,
) -> Result<Value> {
    if input_path == output {
        return Err(invalid("Tokenizer reference output must differ from input"));
    }
    let reference = Reference::open(directory)?;
    let raw = input(input_path)?;
    let text = python_text(&raw)?;
    let lines = text.bytes().filter(|byte| *byte == b'\n').count()
        + usize::from(!text.is_empty() && !text.ends_with('\n'));
    let start = Instant::now();
    let tokens = reference.encode(&text, false)?;
    let elapsed = start.elapsed().as_nanos();
    write_tokens(output, &tokens)?;
    Ok(
        json!({"Source":"tests/test-tokenizer-0.py","ReferenceClass":reference.class,"ReferenceSourceCorrection":reference.source_correction,"SourceAssertions":0,"Completed":true,"InputBytes":raw.len(),"UniversalNewlineTextBytes":text.len(),"LineCount":lines,"TokenCount":tokens.len(),"Output":output,"OutputSha256":super::lora_inputs::digest(output)?.1,"DiagnosticEncodingNanoseconds":elapsed.to_string(),"AddSpecial":false}),
    )
}
fn export_production(
    engine: &Engine,
    vocabulary: &Path,
    raw: &[u8],
    output: &Path,
) -> Result<Option<Value>> {
    let bytes = cpp_text(raw);
    let actions = json!([{"Kind":"clock"},{"Kind":"tokenize","Input":0,"Capture":true,"Detokenize":false},{"Kind":"clock"}]);
    let Some(response)=engine.execute_common("tokenizer-file",&json!({"SchemaVersion":1,"Operations":[{"Kind":"vocab-evaluate","Path":vocabulary,"CreateContext":true,"Inputs":[{"Bytes":bytes,"AddSpecial":false,"ParseSpecial":false}],"Workers":[{"Actions":actions}]}]}))?else{return Ok(None);};
    let results = response["Results"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(|| invalid("Tokenizer file response extent malformed"))?;
    if response["SchemaVersion"] != 1 {
        return Err(invalid("Tokenizer file response schema mismatch"));
    }
    let value = &results[0];
    if !value["Error"].is_null() || value["ModelLoaded"] != true || value["ContextLoaded"] != true {
        return Err(invalid(format!(
            "Tokenizer production load/operation failed: {value}"
        )));
    }
    let workers = value["Workers"]
        .as_array()
        .filter(|v| v.len() == 1)
        .ok_or_else(|| invalid("Tokenizer file worker missing"))?;
    let events = workers[0]["Results"]
        .as_array()
        .filter(|v| v.len() == 3)
        .ok_or_else(|| invalid("Tokenizer file events missing"))?;
    if workers[0]["Completed"] != 3 || !events[1]["TextHex"].is_null() {
        return Err(invalid("Tokenizer file production actions incomplete"));
    }
    let tokens = events[1]["Tokens"]
        .as_array()
        .ok_or_else(|| invalid("Tokenizer file token IDs missing"))?
        .iter()
        .map(|token| {
            Ok(u32::try_from(token.as_u64().ok_or_else(|| {
                invalid("Tokenizer production ID invalid")
            })?)?)
        })
        .collect::<Result<Vec<_>>>()?;
    let before = events[0]["Microseconds"]
        .as_i64()
        .ok_or_else(|| invalid("Tokenizer production clock missing"))?;
    let after = events[2]["Microseconds"]
        .as_i64()
        .ok_or_else(|| invalid("Tokenizer production clock missing"))?;
    let elapsed = after
        .checked_sub(before)
        .filter(|v| *v >= 0)
        .ok_or_else(|| invalid("Tokenizer production clock moved backward"))?;
    write_tokens(output, &tokens)?;
    Ok(Some(
        json!({"Source":"tests/test-tokenizer-0.cpp","Mode":"original-file-export","SourceAssertions":0,"Completed":true,"WindowsTextBytes":bytes.len(),"AddSpecial":false,"ParseSpecial":false,"TokenCount":tokens.len(),"Output":output,"OutputSha256":super::lora_inputs::digest(output)?.1,"DiagnosticMicroseconds":elapsed}),
    ))
}
pub(super) fn compare_file(
    engine: &Engine,
    vocabulary: &Path,
    reference_directory: &Path,
    input_path: &Path,
    output_directory: &Path,
) -> Result<Option<Value>> {
    if !output_directory.is_absolute() || !output_directory.is_dir() {
        return Err(invalid(
            "Tokenizer comparison output directory must exist and be absolute",
        ));
    }
    let raw = input(input_path)?;
    let reference_file = output_directory.join("input.tok");
    let production_file = output_directory.join("input.tokcpp");
    let reference = export_reference(reference_directory, input_path, &reference_file)?;
    let Some(production) = export_production(engine, vocabulary, &raw, &production_file)? else {
        return Ok(None);
    };
    let expected = std::fs::read_to_string(reference_file)?;
    let actual = std::fs::read_to_string(production_file)?;
    let equal = expected == actual;
    let expected = expected.lines().collect::<Vec<_>>();
    let actual = actual.lines().collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    for index in 0..expected.len().max(actual.len()) {
        if expected.get(index) != actual.get(index) {
            if let Some(token) = expected.get(index) {
                diagnostics.push(format!("< row{} {token}", index + 1));
            }
            if diagnostics.len() >= 32 {
                break;
            }
            if let Some(token) = actual.get(index) {
                diagnostics.push(format!("> row{} {token}", index + 1));
            }
            if diagnostics.len() >= 32 {
                break;
            }
        }
    }
    Ok(Some(
        json!({"Source":"tests/test-tokenizer-0.sh","SourceAssertions":0,"WorkloadCompleted":true,"SourceExitSuccess":true,"StrictReferenceMatch":equal,"DifferenceDiagnostics":diagnostics,"DiagnosticFormat":"First32 differing token-file rows; the original GNUdiff output was diagnostic only","Reference":reference,"Production":production,"SourceCorrection":"The retired C++ test executable/build step is replaced by Rust-owned file flow over generic production vocabulary primitives"}),
    ))
}
fn run(engine: &Engine, shell: bool) -> Result<Value> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main")
    {
        return Err(invalid(
            "The complete 26-family tokenizer file workload runs only on main CI",
        ));
    }
    let id = if shell {
        "tests/test-tokenizer-0.sh"
    } else {
        "tests/test-tokenizer-0.py"
    };
    let prepared = super::tokenizer_inputs::prepare(engine)?;
    let selected = engine.source_path("tests/test-tokenizer-0.py")?;
    let evidence = engine.evidence_path(if shell {
        "tokenizer-shell-files"
    } else {
        "tokenizer-reference-files"
    })?;
    let mut cases = Vec::new();
    for case in prepared.cases {
        let directory = evidence.join(&case.name);
        std::fs::create_dir(&directory)?;
        let observation = if shell {
            let Some(value) = compare_file(
                engine,
                &case.vocabulary,
                &case.reference_directory,
                &selected,
                &directory,
            )?
            else {
                return Ok(result(
                    id,
                    false,
                    "production vocabulary API is unavailable".to_owned(),
                ));
            };
            value
        } else {
            export_reference(
                &case.reference_directory,
                &selected,
                &directory.join("input.tok"),
            )?
        };
        cases.push(json!({"Name":case.name,"Observation":observation}));
    }
    let strict = if shell {
        Some(
            cases
                .iter()
                .all(|case| case["Observation"]["StrictReferenceMatch"] == true),
        )
    } else {
        None
    };
    Ok(result(id,true,json!({"SourceAssertions":0,"WorkloadCompleted":true,"StrictReferenceMatch":strict,"HarnessInputSelection":{"SourcePath":"tests/test-tokenizer-0.py","Reason":"Original CLI requires an input; acceptance uses its immutableUTF8 source text for all26 declared tokenizer cases","CallerSuppliedInputApiRetained":true},"Inputs":prepared.receipt,"Cases":cases}).to_string()))
}
pub(super) fn run_python(engine: &Engine) -> Result<Value> {
    run(engine, false)
}
pub(super) fn run_shell(engine: &Engine) -> Result<Value> {
    run(engine, true)
}

pub(super) fn diagnostic(engine: &Engine, value: &Value) -> Result<Value> {
    super::llama_grammar::exact_fields(
        value,
        &["Kind", "Mode", "ReferenceDirectory", "Input", "Vocabulary"],
    )?;
    let path = |key| -> Result<std::path::PathBuf> {
        let path = std::path::PathBuf::from(
            value[key]
                .as_str()
                .ok_or_else(|| invalid("Tokenizer file path must be text"))?,
        );
        if !path.is_absolute() {
            return Err(invalid("Tokenizer file path must be absolute"));
        }
        Ok(path)
    };
    let reference = path("ReferenceDirectory")?;
    let input = path("Input")?;
    let output = engine.evidence_path("tokenizer-file-diagnostic")?;
    let observation = match value["Mode"].as_str() {
        Some("reference") if value["Vocabulary"].is_null() => Some(export_reference(
            &reference,
            &input,
            &output.join("input.tok"),
        )?),
        Some("compare") => compare_file(engine, &path("Vocabulary")?, &reference, &input, &output)?,
        _ => {
            return Err(invalid(
                "Tokenizer file mode requires reference/null-vocabulary or compare/vocabulary",
            ));
        }
    };
    Ok(
        json!({"Diagnostic":"tokenizer-file-options","CorrectnessSourceCount":0,"Observation":observation}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn focused_source_file_newline_guard() -> Result<()> {
        let input = b"a\r\nb\rc\x1aend\r\n";
        assert_eq!(python_text(input)?, "a\nb\nc\u{1a}end\n");
        assert_eq!(cpp_text(input), b"a\nb\rc");
        assert!(python_text(&[0xff]).is_err());
        Ok(())
    }
}
