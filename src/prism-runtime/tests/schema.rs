#![forbid(unsafe_code)]
//! Focused exact production schema conversion regressions from pinned inputs.

#[cfg(feature = "cli")]
#[test]
fn original_schema_conversion_cases() -> bonsai_prism_runtime::Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../examples/upstream_prism/json-schema-cases.json"
    ))?;
    let trim = |text: &str| {
        text.trim_matches([' ', '\n', '\r', '\t'])
            .lines()
            .map(|line| line.trim_start_matches([' ', '\t']))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut failures = Vec::new();
    for section in ["Shared", "CppOnly"] {
        for case in fixture[section].as_array().ok_or("schema cases missing")? {
            let result = bonsai_prism_runtime::schema::to_grammar(
                case["SchemaJson"].as_str().ok_or("schema input missing")?,
                true,
            );
            if case["ExpectedStatus"] == "SUCCESS" {
                match result {
                    Ok(grammar) => {
                        let expected = trim(
                            case["ExpectedGrammar"]
                                .as_str()
                                .ok_or("schema expectation missing")?,
                        );
                        let actual = trim(&grammar);
                        if actual != expected {
                            failures.push(format!(
                                "{}:\nExpected:\n{expected}\nActual:\n{actual}",
                                case["Name"]
                            ));
                        }
                    }
                    Err(error) => failures.push(format!("{}: {error}", case["Name"])),
                }
            } else if result.is_ok() {
                failures.push(format!("{}: expected rejection", case["Name"]));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[cfg(feature = "cli")]
#[test]
fn original_schema_string_probes() -> bonsai_prism_runtime::Result<()> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../examples/upstream_prism/json-schema-cases.json"
    ))?;
    for case in fixture["ResolvesToString"]
        .as_array()
        .ok_or("schema probes missing")?
    {
        let mut info = bonsai_prism_runtime::schema::SchemaInfo::new(
            case["SchemaJson"].as_str().ok_or("schema input missing")?,
        )?;
        info.resolve_refs()?;
        assert_eq!(
            info.resolves_to_string(),
            case["Expected"]
                .as_bool()
                .ok_or("schema probe expectation missing")?,
            "{}",
            case["Name"]
        );
    }
    Ok(())
}
