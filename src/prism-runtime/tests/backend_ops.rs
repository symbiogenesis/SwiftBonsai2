//! Focused CPU graph storage, aliasing, shape, and numerical regressions.

use bonsai_prism_runtime::Result;
use bonsai_prism_runtime::backend::{Argument, CpuBackend, Observation, TensorId};

fn result_tensor(value: &Observation) -> Result<TensorId> {
    match value {
        Observation::Tensor(Some(id)) => Ok(*id),
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
    let transposed = result_tensor(&backend.call(
        "ggml_transpose",
        &[Argument::Context(1), Argument::Tensor(a)],
    )?)?;
    assert_eq!(backend.values(transposed)?, [1.0, 3.0, 5.0, 2.0, 4.0, 6.0]);
    let b = backend.tensor(1, 0, [3, 1, 1, 1])?;
    backend.set_values(b, &[10.0, 20.0, 30.0])?;
    let sum = result_tensor(&backend.call(
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
    let product = result_tensor(&backend.call(
        "ggml_mul_mat",
        &[
            Argument::Context(1),
            Argument::Tensor(a),
            Argument::Tensor(b),
        ],
    )?)?;
    evaluate(&mut backend, product)?;
    assert_eq!(backend.values(product)?, [50.0, 122.0, 68.0, 167.0]);
    let logits = backend.tensor(1, 0, [3, 1, 1, 1])?;
    backend.set_values(logits, &[0.0, 2.0_f32.ln(), 3.0_f32.ln()])?;
    let probabilities = result_tensor(&backend.call(
        "ggml_soft_max",
        &[Argument::Context(1), Argument::Tensor(logits)],
    )?)?;
    evaluate(&mut backend, probabilities)?;
    for (&actual, expected) in
        backend
            .values(probabilities)?
            .iter()
            .zip([1.0 / 6.0, 2.0 / 6.0, 0.5])
    {
        assert!((actual - expected).abs() < 1e-6);
    }
    Ok(())
}

#[test]
fn inplace_views_write_underlying_storage_and_reject_invalid_extent() -> Result<()> {
    let mut backend = CpuBackend::new();
    let a = backend.tensor(1, 0, [4, 1, 1, 1])?;
    backend.set_values(a, &[1.0, 2.0, 3.0, 4.0])?;
    let view = result_tensor(&backend.call(
        "ggml_view_1d",
        &[
            Argument::Context(1),
            Argument::Tensor(a),
            Argument::Integer(2),
            Argument::Integer(4),
        ],
    )?)?;
    let changed = result_tensor(&backend.call(
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

#[test]
fn image_extraction_convolution_and_pooling_preserve_axis_order() -> Result<()> {
    let mut backend = CpuBackend::new();
    let image = backend.tensor(1, 0, [3, 3, 1, 1])?;
    backend.set_values(image, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0])?;
    let kernel = backend.tensor(1, 0, [2, 2, 1, 1])?;
    backend.set_values(kernel, &[1.0; 4])?;
    let mut arguments = vec![
        Argument::Context(1),
        Argument::Tensor(kernel),
        Argument::Tensor(image),
    ];
    arguments.extend([1, 1, 0, 0, 1, 1].map(Argument::Integer));
    let convolution = result_tensor(&backend.call("ggml_conv_2d_direct", &arguments)?)?;
    evaluate(&mut backend, convolution)?;
    assert_eq!(backend.values(convolution)?, [12.0, 16.0, 24.0, 28.0]);
    arguments.extend([Argument::Bool(true), Argument::Integer(0)]);
    let columns = result_tensor(&backend.call("ggml_im2col", &arguments)?)?;
    evaluate(&mut backend, columns)?;
    assert_eq!(
        backend.values(columns)?,
        [
            1.0, 2.0, 4.0, 5.0, 2.0, 3.0, 5.0, 6.0, 4.0, 5.0, 7.0, 8.0, 5.0, 6.0, 8.0, 9.0
        ]
    );
    let maximum = result_tensor(&backend.call(
        "ggml_pool_2d",
        &[
            Argument::Context(1),
            Argument::Tensor(image),
            Argument::Integer(0),
            Argument::Integer(2),
            Argument::Integer(2),
            Argument::Integer(1),
            Argument::Integer(1),
            Argument::Float(0.0),
            Argument::Float(0.0),
        ],
    )?)?;
    evaluate(&mut backend, maximum)?;
    assert_eq!(backend.values(maximum)?, [5.0, 6.0, 8.0, 9.0]);
    Ok(())
}

#[test]
fn interpolation_and_reflection_use_pinned_pixel_positions() -> Result<()> {
    let mut backend = CpuBackend::new();
    let image = backend.tensor(1, 0, [2, 2, 1, 1])?;
    backend.set_values(image, &[1.0, 2.0, 3.0, 4.0])?;
    let enlarged = result_tensor(&backend.call(
        "ggml_upscale",
        &[
            Argument::Context(1),
            Argument::Tensor(image),
            Argument::Integer(2),
            Argument::Integer(1),
        ],
    )?)?;
    evaluate(&mut backend, enlarged)?;
    assert_eq!(
        backend.values(enlarged)?,
        [
            1.0, 1.25, 1.75, 2.0, 1.5, 1.75, 2.25, 2.5, 2.5, 2.75, 3.25, 3.5, 3.0, 3.25, 3.75, 4.0
        ]
    );
    let constant = backend.tensor(1, 0, [4, 4, 1, 1])?;
    backend.set_values(constant, &[1.0; 16])?;
    for mode in [2, 513] {
        let interpolated = result_tensor(&backend.call(
            "ggml_interpolate",
            &[
                Argument::Context(1),
                Argument::Tensor(constant),
                Argument::Integer(2),
                Argument::Integer(2),
                Argument::Integer(1),
                Argument::Integer(1),
                Argument::Integer(mode),
            ],
        )?)?;
        evaluate(&mut backend, interpolated)?;
        assert!(
            backend
                .values(interpolated)?
                .iter()
                .all(|value| (*value - 1.0).abs() < 1e-6)
        );
    }
    let line = backend.tensor(1, 0, [3, 1, 1, 1])?;
    backend.set_values(line, &[1.0, 2.0, 3.0])?;
    let reflected = result_tensor(&backend.call(
        "ggml_pad_reflect_1d",
        &[
            Argument::Context(1),
            Argument::Tensor(line),
            Argument::Integer(2),
            Argument::Integer(1),
        ],
    )?)?;
    evaluate(&mut backend, reflected)?;
    assert_eq!(backend.values(reflected)?, [3.0, 2.0, 1.0, 2.0, 3.0, 2.0]);
    Ok(())
}
