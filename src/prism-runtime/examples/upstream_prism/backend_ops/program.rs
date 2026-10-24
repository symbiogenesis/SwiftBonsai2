// Copyright (c) 2023-2026 The ggml/llama.cpp authors
// SPDX-License-Identifier: MIT
//! Immutable, typed source program. It contains inputs and source expressions, never engine answers.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub(super) const SOURCE_SHA256: &str =
    "8bf7a5cf29d3b3e88aa0c30611f5f1679f46a71ec00306908c44a73ec98ac425";
pub(super) const EVAL_COUNT: usize = 22_817;
pub(super) const PERF_COUNT: usize = 1_530;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "k")]
pub(super) enum Expression {
    #[serde(rename = "literal")]
    Literal {
        value: Value,
        #[serde(rename = "numberKind", default)]
        number_kind: Option<String>,
    },
    #[serde(rename = "bytes")]
    Bytes { bytes: Vec<u8> },
    #[serde(rename = "id")]
    Id { name: String },
    #[serde(rename = "array")]
    Array { values: Vec<Self> },
    #[serde(rename = "unary")]
    Unary { op: String, value: Box<Self> },
    #[serde(rename = "binary")]
    Binary {
        op: String,
        left: Box<Self>,
        right: Box<Self>,
    },
    #[serde(rename = "member")]
    Member { object: Box<Self>, name: String },
    #[serde(rename = "index")]
    Index { object: Box<Self>, index: Box<Self> },
    #[serde(rename = "call")]
    Call { callee: Box<Self>, args: Vec<Self> },
    #[serde(rename = "ternary")]
    Ternary {
        condition: Box<Self>,
        yes: Box<Self>,
        no: Box<Self>,
    },
    #[serde(rename = "cast")]
    Cast {
        #[serde(rename = "type")]
        kind: String,
        value: Box<Self>,
    },
    #[serde(rename = "lambda")]
    Lambda {
        params: Vec<Parameter>,
        body: Vec<Statement>,
    },
    #[serde(rename = "new")]
    New { value: Box<Self> },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "k")]
pub(super) enum Statement {
    #[serde(rename = "empty")]
    Empty,
    #[serde(rename = "decl")]
    Declare {
        name: String,
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        dimensions: Vec<Option<Expression>>,
        value: Option<Expression>,
    },
    #[serde(rename = "decls")]
    Declarations { body: Vec<Self> },
    #[serde(rename = "expr")]
    Expression { value: Expression },
    #[serde(rename = "return")]
    Return { value: Option<Expression> },
    #[serde(rename = "if")]
    If {
        condition: Expression,
        body: Vec<Self>,
        #[serde(default)]
        otherwise: Vec<Self>,
    },
    #[serde(rename = "scope")]
    Scope { body: Vec<Self> },
    #[serde(rename = "for")]
    For {
        initialization: Vec<Self>,
        condition: Expression,
        increment: Expression,
        body: Vec<Self>,
    },
    #[serde(rename = "range-for")]
    RangeFor {
        name: String,
        #[serde(rename = "type")]
        kind: String,
        value: Expression,
        body: Vec<Self>,
    },
    #[serde(rename = "while")]
    While {
        condition: Expression,
        body: Vec<Self>,
    },
    #[serde(rename = "do")]
    Do {
        condition: Expression,
        body: Vec<Self>,
    },
    #[serde(rename = "switch")]
    Switch {
        value: Expression,
        cases: Vec<SwitchCase>,
    },
    #[serde(rename = "break")]
    Break,
    #[serde(rename = "continue")]
    Continue,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct SwitchCase {
    pub(super) labels: Vec<Option<Expression>>,
    pub(super) body: Vec<Statement>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Parameter {
    pub(super) name: String,
    #[serde(rename = "Type")]
    pub(super) kind: String,
    pub(super) default: Option<Expression>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Method {
    pub(super) name: String,
    pub(super) parameters: Vec<Parameter>,
    pub(super) statements: Vec<Statement>,
    #[serde(default)]
    pub(super) source_start: usize,
    #[serde(default)]
    pub(super) source_end: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Initializer {
    pub(super) name: String,
    pub(super) arguments: Vec<Expression>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Constructor {
    pub(super) parameters: Vec<Parameter>,
    pub(super) initializers: Vec<Initializer>,
    pub(super) statements: Vec<Statement>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Class {
    pub(super) name: String,
    pub(super) base: String,
    pub(super) fields: Vec<Statement>,
    pub(super) constructor: Constructor,
    pub(super) methods: Vec<Method>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Program {
    pub(super) schema_version: u32,
    pub(super) source: String,
    pub(super) source_sha256: String,
    pub(super) classes: Vec<Class>,
    pub(super) helpers: Vec<Method>,
    pub(super) constants: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Geometry {
    pub(super) class: String,
    pub(super) arguments: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Geometries {
    pub(super) schema_version: u32,
    pub(super) source: String,
    pub(super) source_sha256: String,
    pub(super) eval_count: usize,
    pub(super) perf_count: usize,
    pub(super) eval_class_count: usize,
    pub(super) perf_class_count: usize,
    pub(super) eval_sha256: String,
    pub(super) perf_sha256: String,
    pub(super) eval: Vec<Geometry>,
    pub(super) perf: Vec<Geometry>,
}

pub(super) fn load() -> Result<(&'static Program, &'static Geometries), String> {
    static SOURCE: OnceLock<Result<(Program, Geometries), String>> = OnceLock::new();
    let loaded = SOURCE.get_or_init(|| {
        let program: Program = serde_json::from_str(include_str!("../backend-ops-program.json"))
            .map_err(|error| format!("backend source program: {error}"))?;
        let geometries: Geometries =
            serde_json::from_str(include_str!("../backend-ops-geometries.json"))
                .map_err(|error| format!("backend source geometry: {error}"))?;
        if program.schema_version != 1
            || geometries.schema_version != 1
            || program.source != "tests/test-backend-ops.cpp"
            || geometries.source != program.source
            || program.source_sha256 != SOURCE_SHA256
            || geometries.source_sha256 != SOURCE_SHA256
            || geometries.eval_count != EVAL_COUNT
            || geometries.eval.len() != EVAL_COUNT
            || geometries.perf_count != PERF_COUNT
            || geometries.perf.len() != PERF_COUNT
            || geometries.eval_class_count != 112
            || geometries.perf_class_count != 38
            || geometries.eval_sha256
                != "1724d4a88170deb030f676d81f97ce050ab6ee32c37452626fd834ea1c4f7419"
            || geometries.perf_sha256
                != "f2dc5bef74e18a5e9740ed77abe9ffd6f5af2ec36c7d00e56d7128bee3205674"
        {
            return Err("backend source/geometry provenance or extent changed".into());
        }
        let classes: BTreeSet<_> = program.classes.iter().map(|class| &class.name).collect();
        if classes.len() != 114
            || program
                .classes
                .iter()
                .map(|class| class.methods.len())
                .sum::<usize>()
                != 328
            || program.helpers.len() != 7
            || geometries
                .eval
                .iter()
                .any(|case| !classes.contains(&case.class))
            || geometries
                .perf
                .iter()
                .any(|case| !classes.contains(&case.class))
            || geometries
                .eval
                .iter()
                .map(|case| &case.class)
                .collect::<BTreeSet<_>>()
                .len()
                != 112
            || geometries
                .perf
                .iter()
                .map(|case| &case.class)
                .collect::<BTreeSet<_>>()
                .len()
                != 38
        {
            return Err("backend source class/method inventory is incomplete".into());
        }
        Ok((program, geometries))
    });
    match loaded {
        Ok((program, geometries)) => Ok((program, geometries)),
        Err(error) => Err(error.clone()),
    }
}
