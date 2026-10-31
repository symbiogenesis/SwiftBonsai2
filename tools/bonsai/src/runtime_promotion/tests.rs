use super::*;

fn policy_fixture() -> crate::Result<Value> {
    policy(&"a".repeat(40), 1)
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
    assert_eq!(policy["identities"]["nativeRowWorkers"], 1);
    assert_eq!(
        policy["identities"]["servingStatePolicy"],
        "reset-every-request"
    );
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
fn row_worker_evidence_cannot_qualify_a_different_execution_policy() -> crate::Result<()> {
    let single = policy_fixture()?;
    let parallel = policy(&"a".repeat(40), 4)?;
    let plan = run(json!({
        "operation": "plan", "runnerRevision": "a".repeat(40), "nativeRowWorkers": 4
    }))?;
    assert_eq!(plan["policy"], parallel);
    assert_eq!(parallel["identities"]["nativeRowWorkers"], 4);
    assert!(score(evidence(&single)?, &parallel, &std::env::temp_dir()).is_err());
    assert!(score(evidence(&parallel)?, &single, &std::env::temp_dir()).is_err());
    for value in [
        json!(0),
        json!(2),
        json!(8),
        json!(256),
        json!(-1),
        json!("4"),
        Value::Null,
    ] {
        assert!(
            run(json!({
                "operation": "plan", "runnerRevision": "a".repeat(40), "nativeRowWorkers": value
            }))
            .is_err()
        );
    }
    let mut reused = evidence(&parallel)?;
    reused["identities"]["servingStatePolicy"] = json!("same-session-live-prefix");
    assert!(score(reused, &parallel, &std::env::temp_dir()).is_err());
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
        let mut x2 = input.clone();
        for record in x2["gates"]
            .as_array_mut()
            .ok_or("Missing promotion gates.")?
        {
            if GATES
                .iter()
                .any(|gate| gate.device && record["id"] == gate.id)
            {
                record["execution"]["deviceModel"] = json!("Snapdragon X2 Elite Extreme");
                record["execution"]["processorName"] =
                    json!("Snapdragon(R) X2 Elite Extreme - X2E-96-100");
            }
        }
        let x2_result = score(x2.clone(), &policy, &directory)?;
        assert_eq!(x2_result["status"], "passed");
        assert_eq!(
            x2_result["gates"][4]["deviceModel"],
            "Snapdragon X2 Elite Extreme"
        );
        x2["gates"][4]["execution"] = input["gates"][4]["execution"].clone();
        assert!(
            score(x2, &policy, &directory).is_err(),
            "Mixed X1/X2 device evidence was accepted."
        );
        let mut mislabeled = input.clone();
        mislabeled["gates"][4]["execution"]["processorName"] = json!("Snapdragon X2 Elite Extreme");
        assert!(score(mislabeled, &policy, &directory).is_err());
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
