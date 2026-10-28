use super::*;

fn policy_fixture() -> crate::Result<Value> {
    policy(&"a".repeat(40))
}

fn evidence(policy: &Value) -> crate::Result<Value> {
    Ok(
        json!({"schemaVersion": 1, "policySha256": sha256(&serde_json::to_vec(policy).map_err(|error| error.to_string())?), "identities": policy["identities"], "gates": []}),
    )
}

#[test]
fn missing_evidence_stays_pending_and_policy_binds_standard_profile() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let result = score(evidence(&policy)?, &policy, &std::env::temp_dir())?;
    assert_eq!(result["status"], "pending");
    assert_eq!(result["promotionReady"], false);
    assert_eq!(result["gates"].as_array().map(Vec::len), Some(GATES.len()));
    assert_eq!(policy["identities"]["adapterScale"], 1.3);
    assert_eq!(policy["identities"]["contextTokens"], 8192);
    assert_eq!(policy["identities"]["maxCompletionTokens"], 4096);
    assert!(
        policy["identities"]["adapterSha256"]
            .as_str()
            .is_some_and(|id| is_lower_hex(id, 64))
    );
    let plan = run(json!({"operation": "plan", "runnerRevision": "a".repeat(40)}))?;
    assert_eq!(plan["policy"], policy);
    assert_eq!(
        score(
            plan["evidenceTemplate"].clone(),
            &policy,
            &std::env::temp_dir()
        )?["status"],
        "pending"
    );
    Ok(())
}

#[test]
fn stale_artifacts_unknown_duplicate_and_array_records_reject() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let mut input = evidence(&policy)?;
    input["identities"]["adapterScale"] = json!(1);
    assert!(score(input, &policy, &std::env::temp_dir()).is_err());
    for records in [
        json!([[]]),
        json!([{"id":"unknown", "status":"pending", "note":"pending"}]),
        json!([{"id":"runner-foundation", "status":"pending", "note":"pending"},{"id":"runner-foundation", "status":"pending", "note":"pending"}]),
    ] {
        let mut input = evidence(&policy)?;
        input["gates"] = records;
        assert!(score(input, &policy, &std::env::temp_dir()).is_err());
    }
    Ok(())
}

#[test]
fn reviewed_pass_requires_report_device_main_and_human_identity() -> crate::Result<()> {
    let policy = policy_fixture()?;
    let directory = std::env::temp_dir().join(format!(
        "Prism-Promotion-Test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
    let report = b"retained reviewed report";
    std::fs::write(directory.join("report.json"), report).map_err(|error| error.to_string())?;
    let mut input = evidence(&policy)?;
    input["gates"] = json!(GATES.iter().map(|gate| json!({"id":gate.id, "status":"passed", "note":"Review checked the exact retained report and required scope.", "report":{"file":"report.json","sha256":sha256(report)}, "execution":{"repository":gate.repository,"revision":if gate.repository == PRISM {policy["identities"]["runtimeRevision"].clone()} else {policy["identities"]["runnerRevision"].clone()},"gitRef":"refs/heads/main","runId":"123","runAttempt":1,"architecture":"ARM64","deviceModel":"Snapdragon X Elite","selfHosted":true},"reviewerId":"external-reviewer","humanReviewed":gate.human})).collect::<Vec<_>>());
    let result = (|| {
        assert_eq!(
            score(input.clone(), &policy, &directory)?["status"],
            "passed"
        );
        let manifest = directory.join("promotion.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&input).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            run(
                json!({"operation": "file-score", "runnerRevision": "a".repeat(40), "evidencePath": manifest})
            )?["promotionReady"],
            true
        );
        for (pointer, value) in [
            (
                "/gates/4/execution/deviceModel",
                json!("generic hosted ARM64"),
            ),
            ("/gates/4/execution/gitRef", json!("refs/heads/work")),
            ("/gates/4/reviewerId", Value::Null),
            ("/gates/10/humanReviewed", json!(false)),
            ("/gates/4/report/file", json!("../report.json")),
            ("/gates/4/report/sha256", json!("0".repeat(64))),
        ] {
            let mut changed = input.clone();
            *changed.pointer_mut(pointer).ok_or("Missing test field.")? = value;
            assert!(score(changed, &policy, &directory).is_err(), "{pointer}");
        }
        input["gates"][4]["status"] = json!("rejected");
        input["gates"][5]["status"] = json!("pending");
        assert_eq!(score(input, &policy, &directory)?["status"], "rejected");
        Ok(())
    })();
    std::fs::remove_file(directory.join("report.json")).map_err(|error| error.to_string())?;
    std::fs::remove_file(directory.join("promotion.json")).map_err(|error| error.to_string())?;
    std::fs::remove_dir(&directory).map_err(|error| error.to_string())?;
    result
}
