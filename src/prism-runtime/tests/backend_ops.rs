//! Focused CPU graph storage, aliasing, shape, and numerical regressions.

use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::backend::{Argument, CpuBackend, Observation, TensorId};

fn result_tensor(value: Observation) -> Result<TensorId> {
    match value {
        Observation::Tensor(Some(id)) => Ok(id),
        _ => Err(std::io::Error::other("expected tensor result").into()),
    }
}

fn evaluate(backend: &mut CpuBackend, tensor: TensorId) -> Result<()> {
    let Observation::Graph(graph) = backend.call("ggml_new_graph", &[Argument::Context(1)])? else {
        return Err(std::io::Error::other("expected graph result").into());
    };
    backend.call(
        "ggml_build_forward_expand",
        &[Argument::Graph(graph), Argument::Tensor(tensor)],
    )?;
    backend.compute(graph)
}

#[test]
fn strided_transpose_and_broadcast_preserve_logical_values() -> Result<()> {
    let mut backend = CpuBackend::new();
    let a = backend.tensor(1, 0, [2, 3, 1, 1])?;
    backend.set_values(a, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])?;
    let transposed = result_tensor(backend.call(
        "ggml_transpose",
        &[Argument::Context(1), Argument::Tensor(a)],
    )?)?;
    assert_eq!(backend.values(transposed)?, [1.0, 3.0, 5.0, 2.0, 4.0, 6.0]);
    let b = backend.tensor(1, 0, [3, 1, 1, 1])?;
    backend.set_values(b, &[10.0, 20.0, 30.0])?;
    let sum = result_tensor(backend.call(
        "ggml_add",
        &[
            Argument::Context(1),
            Argument::Tensor(transposed),
            Argument::Tensor(b),
        ],
    )?)?;
    evaluate(&mut backend, sum)?;
    assert_eq!(backend.values(sum)?, [11.0, 23.0, 35.0, 12.0, 24.0, 36.0]);
    assert_eq!(backend.values(a)?, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    Ok(())
}

#[test]
fn matmul_and_softmax_have_independent_expected_results() -> Result<()> {
    let mut backend = CpuBackend::new();
    let a = backend.tensor(1, 0, [3, 2, 1, 1])?;
    let b = backend.tensor(1, 0, [3, 2, 1, 1])?;
    backend.set_values(a, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0])?;
    backend.set_values(b, &[7.0, 8.0, 9.0, 10.0, 11.0, 12.0])?;
    let c = result_tensor(backend.call(
        "ggml_mul_mat",
        &[
            Argument::Context(1),
            Argument::Tensor(a),
            Argument::Tensor(b),
        ],
    )?)?;
    evaluate(&mut backend, c)?;
    assert_eq!(backend.values(c)?, [50.0, 122.0, 68.0, 167.0]);
    let x = backend.tensor(1, 0, [3, 1, 1, 1])?;
    backend.set_values(x, &[0.0, 2.0_f32.ln(), 3.0_f32.ln()])?;
    let y = result_tensor(backend.call(
        "ggml_soft_max",
        &[Argument::Context(1), Argument::Tensor(x)],
    )?)?;
    evaluate(&mut backend, y)?;
    for (&actual, expected) in backend.values(y)?.iter().zip([1.0 / 6.0, 2.0 / 6.0, 0.5]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    Ok(())
}

#[test]
fn inplace_views_write_underlying_storage_and_reject_invalid_extent() -> Result<()> {
    let mut backend = CpuBackend::new();
    let a = backend.tensor(1, 0, [4, 1, 1, 1])?;
    backend.set_values(a, &[1.0, 2.0, 3.0, 4.0])?;
    let view = result_tensor(backend.call(
        "ggml_view_1d",
        &[
            Argument::Context(1),
            Argument::Tensor(a),
            Argument::Integer(2),
            Argument::Integer(4),
        ],
    )?)?;
    let changed = result_tensor(backend.call(
        "ggml_scale_bias_inplace",
        &[
            Argument::Context(1),
            Argument::Tensor(view),
            Argument::Float(2.0),
            Argument::Float(1.0),
        ],
    )?)?;
    evaluate(&mut backend, changed)?;
    assert_eq!(backend.values(a)?, [1.0, 5.0, 7.0, 4.0]);
    assert!(backend.set(view, 8, &[1]).is_err());
    assert!(
        backend
            .call(
                "ggml_mul_mat",
                &[
                    Argument::Context(1),
                    Argument::Tensor(a),
                    Argument::Tensor(view)
                ]
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn unsupported_operation_never_reports_support_or_computes() -> Result<()> {
    let mut backend = CpuBackend::new();
    let tensor = backend.tensor(1, 0, [1; 4])?;
    backend.info_mut(tensor)?.operation = 86;
    assert!(!backend.supports(tensor)?);
    assert!(evaluate(&mut backend, tensor).is_err());
    assert!(
        backend
            .call(
                "ggml_gated_delta_net",
                &[Argument::Context(1), Argument::Tensor(tensor)]
            )
            .is_err()
    );
    Ok(())
}
