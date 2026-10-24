//! Rust-owned CPU barrier workload; the source has no numeric output assertions.
//! Every execution is CI-only because the original includes timed graph work.
use super::engine::Engine;
use super::{Result, invalid, result};
use serde_json::{Value, json};
use std::env;
use std::time::Instant;

const ID: &str = "tests/test-barrier.cpp";

fn graph(width: usize, repetitions: usize) -> Value {
    let mut nodes = vec![json!({"Op":"input","Type":"F32","Dimensions":[width]})];
    let mut output = 0;
    for _ in 0..repetitions {
        let a = nodes.len();
        nodes.push(json!({"Op":"input","Type":"Q4_0","Dimensions":[width,128]}));
        output = nodes.len();
        nodes.push(json!({"Op":"mul_mat","A":a,"B":output-2}));
        let d = nodes.len();
        nodes.push(json!({"Op":"input","Type":"Q4_0","Dimensions":[128,width]}));
        let previous = output;
        output = nodes.len();
        nodes.push(json!({"Op":"mul_mat","A":d,"B":previous}));
    }
    json!({"Nodes":nodes,"Output":output})
}

fn plan(slot: usize, graph: usize, threads: usize) -> Value {
    json!({"Op":"plan","Slot":slot,"Graph":graph,"Threads":threads})
}

fn schedule(kind: usize, threads: usize, rounds: usize) -> (Value, usize, usize) {
    let mut actions = Vec::new();
    let (graphs, plans, computes) = match kind {
        0 => {
            actions.push(plan(0, 0, threads));
            actions.push(json!({"Op":"compute","Slot":0})); // Original untimed warmup.
            actions.push(json!({"Op":"timer-start"}));
            actions.extend((0..rounds).map(|_| json!({"Op":"compute","Slot":0})));
            actions.push(json!({"Op":"timer-stop"}));
            (vec![graph(64, 1000)], 1, rounds + 1)
        }
        1 => {
            for i in 0..rounds * 100 {
                actions.push(plan(0, 0, if i % 4 == 0 { 1 } else { threads }));
                actions.push(json!({"Op":"compute","Slot":0}));
            }
            (vec![graph(64, 2)], rounds * 100, rounds * 100)
        }
        _ => {
            for i in 0..rounds * 10 {
                let count = if i % 4 == 0 { 1 } else { threads };
                // Both plans/workspaces coexist before either graph computes.
                actions.push(plan(0, 0, count));
                actions.push(plan(1, 1, count));
                actions.push(json!({"Op":"compute","Slot":0}));
                actions.push(json!({"Op":"compute","Slot":1}));
            }
            (vec![graph(64, 2), graph(256, 4)], rounds * 20, rounds * 20)
        }
    };
    (
        json!({"Kind":"cpu-threadpool","ContextBytes":1_u64<<30,"PoolThreads":threads,"Graphs":graphs,"Actions":actions}),
        plans,
        computes,
    )
}

pub(super) fn run_with_parameters(engine: &Engine, threads: usize, rounds: usize) -> Result<Value> {
    if env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || env::var("GITHUB_REF").as_deref() != Ok("refs/heads/main")
    {
        return Err(invalid(
            "The timed barrier source runs only in GitHub Actions on main",
        ));
    }
    if !(1..=256).contains(&threads) || !(1..=125).contains(&rounds) {
        return Err(invalid(
            "barrier parameters exceed bounded original workload transport",
        ));
    }
    let mut observations = Vec::new();
    for (kind, name) in ["constant-threads", "changing-threads", "multiple-graphs"]
        .into_iter()
        .enumerate()
    {
        let (operation, plans, computes) = schedule(kind, threads, rounds);
        let started = Instant::now();
        let Some(response) =
            engine.execute_common(name, &json!({"SchemaVersion":1,"Operations":[operation]}))?
        else {
            return Ok(result(ID, false, "Rust runtime has no production graph/threadpool API for the original barrier workload".into()));
        };
        if response["SchemaVersion"] != 1 || response["Results"].as_array().map(Vec::len) != Some(1)
        {
            return Err(invalid("barrier response framing differs"));
        }
        let value = &response["Results"][0];
        super::llama_grammar::exact_fields(
            value,
            &["GraphNodeCounts", "Plans", "ComputeStatuses", "Nanoseconds"],
        )?;
        let expected_nodes = if kind == 0 {
            vec![2000]
        } else if kind == 1 {
            vec![4]
        } else {
            vec![4, 8]
        };
        let statuses = value["ComputeStatuses"]
            .as_array()
            .ok_or_else(|| invalid("barrier compute statuses missing"))?;
        if value["GraphNodeCounts"] != json!(expected_nodes)
            || value["Plans"] != plans
            || statuses.len() != computes
            || statuses.iter().any(|status| status.as_i64() != Some(0))
            || value["Nanoseconds"].as_array().map(Vec::len) != Some(usize::from(kind == 0))
        {
            return Ok(result(
                ID,
                false,
                format!("{name}: additional workload completion guard failed: {value}"),
            ));
        }
        observations.push(json!({"Name":name,"Plans":plans,"Computes":computes,"GraphNodes":expected_nodes,"NativeNanoseconds":value["Nanoseconds"],"WallNanosecondsIncludingTransport":started.elapsed().as_nanos().to_string()}));
    }
    Ok(result(ID, true, json!({"SourceSha256":"44a60801134d178c8e2c6789c471909ed83c0b8c71055b259478b4d396220cee","Category":"CI-only timed workload completion","OriginalNumericAssertions":0,"AdditionalGuards":"graph extent, exact supplied plan/compute counts, production success statuses","Threads":threads,"Rounds":rounds,"InputInitialization":"source leaves GGML tensor data unspecified; no output values are read","TimingThreshold":null,"Results":observations}).to_string()))
}

pub(super) fn run(engine: &Engine) -> Result<Value> {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, 4);
    run_with_parameters(engine, threads, 100)
}

#[cfg(test)]
mod tests {
    #[test]
    fn original_graph_edges_and_switch_lifetimes_are_retained() {
        let graph = super::graph(64, 2);
        assert_eq!(graph["Nodes"][2]["B"], 0);
        assert_eq!(graph["Nodes"][6]["B"], 4);
        assert_eq!(graph["Output"], 8);
        let (operation, plans, computes) = super::schedule(2, 4, 1);
        assert_eq!((plans, computes), (20, 20));
        assert_eq!(operation["Actions"][0]["Threads"], 1);
        assert_eq!(operation["Actions"][1]["Threads"], 1);
        assert_eq!(operation["Actions"][4]["Threads"], 4);
        assert_eq!(operation["Actions"][16]["Threads"], 1);
    }
}
