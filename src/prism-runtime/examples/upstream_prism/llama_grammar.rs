//! Safe Rust ownership of all test-llama-grammar.cpp literal assertions.
//!
//! Source: PrismML-Eng/llama.cpp b87ca7225456b06b0975b8239c66b5aecf07e37c;
//! see tools/prism/upstream/LICENSE for MIT terms. Rust owns every symbol,
//! rule, expected stack element and rejected candidate pair. Native production
//! calls return raw rule storage, stack pointers and candidate observations.

use serde_json::{Value, json};

use super::engine::Engine;
use super::{Result, invalid, result};

const ID: &str = "tests/test-llama-grammar.cpp";
const SOURCE_SHA256: &str = "18405dcd2ff94a2625e8d6e884ac74b704fc60f9c039dbb270ca550f6b2dad76";
const CASE: &str = include_str!("llama-grammar-cases.json");

pub(super) fn field<'a>(value: &'a Value, name: &str) -> Result<&'a Value> {
    value
        .get(name)
        .ok_or_else(|| invalid(format!("missing grammar field {name}")))
}

pub(super) fn array<'a>(value: &'a Value, name: &str) -> Result<&'a [Value]> {
    field(value, name)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| invalid(format!("grammar {name} must be an array")))
}

pub(super) fn uint(value: &Value, name: &str) -> Result<u64> {
    field(value, name)?
        .as_u64()
        .ok_or_else(|| invalid(format!("grammar {name} must be unsigned")))
}

pub(super) fn exact_fields(value: &Value, fields: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("grammar result must be an object"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(invalid("grammar result field mismatch"));
    }
    Ok(())
}

pub(super) fn exchange(engine: &Engine, operations: &[Value]) -> Result<Option<Vec<Value>>> {
    let mut output = Vec::with_capacity(operations.len());
    for operations in operations.chunks(8) {
        let Some(response) = engine.execute_common(
            "grammar",
            &json!({"SchemaVersion":1,"Operations":operations}),
        )?
        else {
            return Ok(None);
        };
        exact_fields(&response, &["SchemaVersion", "Results"])?;
        if uint(&response, "SchemaVersion")? != 1 {
            return Err(invalid("grammar common response schema mismatch"));
        }
        let results = array(&response, "Results")?;
        if results.len() != operations.len() {
            return Err(invalid("grammar operation count mismatch"));
        }
        output.extend_from_slice(results);
    }
    Ok(Some(output))
}

fn pair(value: &Value) -> Result<(u64, u64)> {
    let pair = value
        .as_array()
        .ok_or_else(|| invalid("grammar literal pair must be an array"))?;
    if pair.len() != 2 {
        return Err(invalid("grammar literal pair must have two entries"));
    }
    Ok((
        pair[0]
            .as_u64()
            .ok_or_else(|| invalid("grammar pair type must be unsigned"))?,
        pair[1]
            .as_u64()
            .ok_or_else(|| invalid("grammar pair value must be unsigned"))?,
    ))
}

fn rules(fixture: &Value) -> Result<Value> {
    Ok(json!(
        array(fixture, "Rules")?
            .iter()
            .map(|rule| {
                rule.as_array()
                    .ok_or_else(|| invalid("grammar rule must be an array"))?
                    .iter()
                    .map(|element| {
                        let (kind, value) = pair(element)?;
                        Ok(json!({"Type":kind,"Value":value}))
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?
    ))
}

/// Finds a stack pointer in the native rule storage using integer addresses.
/// The bridge never needs to dereference possibly stale cloned-stack pointers.
pub(super) fn pointed_element(snapshot: &Value, address: u64) -> Result<Option<(u64, u64)>> {
    let rules = array(snapshot, "Rules")?;
    let addresses = array(snapshot, "RuleAddresses")?;
    let element_size = uint(snapshot, "ElementSize")?;
    if addresses.len() != rules.len() || element_size == 0 {
        return Err(invalid("grammar snapshot storage metadata mismatch"));
    }
    for (rule, start) in rules.iter().zip(addresses) {
        let rule = rule
            .as_array()
            .ok_or_else(|| invalid("grammar snapshot rule must be an array"))?;
        let start = start
            .as_u64()
            .ok_or_else(|| invalid("grammar rule address must be unsigned"))?;
        if let Some(delta) = address.checked_sub(start) {
            let offset = delta / element_size;
            if delta.is_multiple_of(element_size) && offset < u64::try_from(rule.len())? {
                let element = &rule[usize::try_from(offset)?];
                exact_fields(element, &["Type", "Value"])?;
                return Ok(Some((uint(element, "Type")?, uint(element, "Value")?)));
            }
        }
    }
    Ok(None)
}

fn root(fixture: &Value) -> Result<u64> {
    let symbols = array(fixture, "Symbols")?;
    if symbols.len() != 13 {
        return Err(invalid("all 13 original grammar symbols are required"));
    }
    for symbol in symbols {
        let symbol = symbol
            .as_array()
            .ok_or_else(|| invalid("grammar symbol must be a pair"))?;
        if symbol.len() != 2 {
            return Err(invalid("grammar symbol must have two entries"));
        }
        if symbol[0].as_str() == Some("root") {
            return symbol[1]
                .as_u64()
                .ok_or_else(|| invalid("root symbol ID must be unsigned"));
        }
    }
    Err(invalid("original root grammar symbol is missing"))
}

fn operation(fixture: &Value) -> Result<Value> {
    let candidates:Vec<Value>=(0..23).map(|index|json!({"Index":index,"CodePoints":[37+index,0],"PartialUtf8":{"Value":0,"NRemain":0},"Id":0})).collect();
    let mut actions = vec![
        json!({"Kind":"init-rules","Grammar":0,"Rules":rules(fixture)?,"StartRule":root(fixture)?}),
        json!({"Kind":"snapshot","Grammar":0}),
        // Preserve the original redundant first rejection call as well.
        json!({"Kind":"reject-candidates","Grammar":0,"Stack":0,"Candidates":candidates}),
    ];
    for stack in 0..8 {
        actions.push(
            json!({"Kind":"reject-candidates","Grammar":0,"Stack":stack,"Candidates":candidates}),
        );
    }
    actions.push(json!({"Kind":"free","Grammar":0}));
    Ok(json!({"Kind":"grammar-trace","Actions":actions}))
}

fn stack_assertions(
    fixture: &Value,
    snapshot: &Value,
    failures: &mut Vec<String>,
) -> Result<usize> {
    exact_fields(
        snapshot,
        &["Rules", "RuleAddresses", "ElementSize", "Stacks"],
    )?;
    let stacks = array(snapshot, "Stacks")?;
    let expected = array(fixture, "ExpectedStacks")?;
    if stacks.len() > expected.len() {
        return Err(invalid(
            "grammar returned more stacks than the original literal table",
        ));
    }
    let mut count = 0;
    for (index, (stack, expected)) in stacks.iter().zip(expected).enumerate() {
        let stack = stack
            .as_array()
            .ok_or_else(|| invalid("grammar stack must be an array"))?;
        let expected = expected
            .as_array()
            .ok_or_else(|| invalid("expected grammar stack must be an array"))?;
        if stack.len() > expected.len() {
            return Err(invalid("grammar stack exceeds its original literal table"));
        }
        for (offset, (address, expected)) in stack.iter().zip(expected).enumerate() {
            let address = address
                .as_u64()
                .ok_or_else(|| invalid("stack pointer address must be unsigned"))?;
            count += 1;
            if pointed_element(snapshot, address)? != Some(pair(expected)?) {
                failures.push(format!(
                    "stack {index} element {offset}: type/value mismatch"
                ));
            }
        }
    }
    Ok(count)
}

fn reject_assertions(
    fixture: &Value,
    observations: &[Value],
    failures: &mut Vec<String>,
) -> Result<usize> {
    let expected = array(fixture, "ExpectedRejects")?;
    let mut count = 0;
    for (stack, (value, expected)) in observations.iter().zip(expected).enumerate() {
        exact_fields(value, &["Rejected"])?;
        let rejected = array(value, "Rejected")?;
        let expected = expected
            .as_array()
            .ok_or_else(|| invalid("expected rejects must be an array"))?;
        if rejected.len() > expected.len() {
            return Err(invalid(
                "grammar rejection extent exceeds original literal table",
            ));
        }
        // Upstream iterates the actual rejection count. Keep its exact prefix
        // contract: six literal rows have a never-read trailing pair (23,60),
        // because the input has only 23 candidates with indices 0 through 22.
        for (index, (candidate, expected)) in rejected.iter().zip(expected).enumerate() {
            exact_fields(candidate, &["Index", "CodePoints", "PartialUtf8", "Id"])?;
            let points = array(candidate, "CodePoints")?;
            let first = points
                .first()
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("rejected candidate must retain its code point"))?;
            count += 1;
            if (uint(candidate, "Index")?, first) != pair(expected)? {
                failures.push(format!(
                    "stack {stack} rejection {index}: index/code point mismatch"
                ));
            }
        }
    }
    Ok(count)
}

pub(super) fn run(engine: &Engine) -> Result<Value> {
    let fixture: Value = serde_json::from_str(CASE)?;
    if uint(&fixture, "SchemaVersion")? != 1
        || field(&fixture, "Source")?.as_str() != Some(ID)
        || field(&fixture, "SourceSha256")?.as_str() != Some(SOURCE_SHA256)
    {
        return Err(invalid("llama-grammar source provenance mismatch"));
    }
    let request = operation(&fixture)?;
    let Some(values) = exchange(engine, std::slice::from_ref(&request))? else {
        return Ok(result(ID,false,"unsupported production API: grammar rules, stack storage and candidate rejection; all original literal contracts required".to_owned()));
    };
    exact_fields(&values[0], &["Trace"])?;
    let observations = array(&values[0], "Trace")?;
    if observations.len() != array(&request, "Actions")?.len() {
        return Err(invalid("grammar trace extent mismatch"));
    }
    exact_fields(&observations[0], &["Created"])?;
    if field(&observations[0], "Created")?.as_bool() != Some(true) {
        return Err(invalid("original literal grammar failed to initialize"));
    }
    let mut failures = Vec::new();
    let stacks = stack_assertions(&fixture, &observations[1], &mut failures)?;
    let rejects = reject_assertions(&fixture, &observations[3..11], &mut failures)?;
    if stacks != 32 || rejects != 142 {
        return Err(invalid(
            "native grammar trace omitted original assertion positions",
        ));
    }
    Ok(result(
        ID,
        failures.is_empty(),
        format!(
            "13 original symbols/rules; 8 stack/rejection prefixes; stack_conditions={stacks}; reject_conditions={rejects}; original baseline=32+142=174 conditions; unused literal rejection tails retained; failed={}; {}",
            failures.len(),
            failures.join("; ")
        ),
    ))
}
