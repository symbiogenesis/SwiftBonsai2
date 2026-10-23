#![forbid(unsafe_code)]
//! Recurrent-state ownership regressions independent of a model artifact.

use bonsai_prism_runtime::{
    Decoder, Result,
    runtime::{Cancellation, Finish, Generation, Limits, Runtime, greedy},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Recurrent {
    tokens: Vec<u32>,
    steps: Rc<Cell<usize>>,
    fail: Rc<Cell<bool>>,
    restore_fail: Rc<Cell<bool>>,
}

impl Decoder for Recurrent {
    type Checkpoint = Vec<u32>;

    fn identity(&self) -> &'static str {
        "recurrent-fixture/f32/fp16-kv/v1"
    }
    fn vocab_size(&self) -> usize {
        8
    }
    fn checkpoint_bytes(&self) -> usize {
        self.tokens.len() * 4
    }
    fn reset(&mut self) -> Result<()> {
        self.tokens.clear();
        Ok(())
    }
    fn evaluate(&mut self, token: u32) -> Result<Vec<f32>> {
        self.steps.set(self.steps.get() + 1);
        self.tokens.push(token);
        if self.fail.replace(false) {
            return Err(std::io::Error::other("injected failure after state mutation").into());
        }
        let next = usize::try_from(self.tokens.iter().sum::<u32>() % 8)?;
        let mut logits = vec![-1.0; 8];
        logits[next] = 1.0;
        Ok(logits)
    }
    fn checkpoint(&self) -> Result<Self::Checkpoint> {
        Ok(self.tokens.clone())
    }
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<()> {
        if self.restore_fail.get() {
            return Err(std::io::Error::other("injected restore failure").into());
        }
        self.tokens.clone_from(checkpoint);
        Ok(())
    }
}

fn decoder() -> Recurrent {
    Recurrent {
        tokens: Vec::new(),
        steps: Rc::new(Cell::new(0)),
        fail: Rc::new(Cell::new(false)),
        restore_fail: Rc::new(Cell::new(false)),
    }
}

const LIMITS: Limits = Limits {
    context_tokens: 32,
    checkpoints: 4,
    checkpoint_bytes: 16_384,
};

const fn options() -> Generation {
    Generation {
        max_tokens: 1,
        stop_tokens: vec![],
    }
}

#[test]
fn edited_turns_restore_complete_recurrent_prefix_and_failed_stream_rolls_back() -> Result<()> {
    let decoder = decoder();
    let steps = decoder.steps.clone();
    let fail = decoder.fail.clone();
    let mut runtime = Runtime::new(decoder, LIMITS)?;
    let cancel = Cancellation::default();
    let first = runtime.generate(&[1], &options(), &cancel, |_| Ok(()))?;
    assert_eq!(first.generated_tokens, 1);
    assert_eq!(runtime.tokens(), &[1, 1]);
    runtime.generate(&[1, 1, 2], &options(), &cancel, |_| Ok(()))?;
    assert_eq!(runtime.tokens(), &[1, 1, 2, 4]);
    assert_eq!(steps.get(), 4);
    let edited = runtime.generate(&[1, 1, 3], &options(), &cancel, |_| Ok(()))?;
    assert_eq!(edited.reused_tokens, 2);
    assert_eq!(runtime.tokens(), &[1, 1, 3, 5]);
    assert_eq!(steps.get(), 6);

    let before = runtime.tokens().to_vec();
    let cancelled = Cancellation::default();
    assert!(
        runtime
            .generate(&[7, 2], &options(), &cancelled, |_| {
                cancelled.cancel();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(runtime.tokens(), before);
    assert!(!runtime.is_faulted());

    fail.set(true);
    assert!(
        runtime
            .generate(&[6], &options(), &cancel, |_| Ok(()))
            .is_err()
    );
    assert_eq!(runtime.tokens(), before);
    let restored = runtime.generate(&[1, 1, 3, 5, 1], &options(), &cancel, |_| Ok(()))?;
    assert_eq!(restored.reused_tokens, 4);
    assert_eq!(runtime.tokens(), &[1, 1, 3, 5, 1, 3]);
    Ok(())
}

#[test]
fn restoration_failure_closes_admission_until_reset() -> Result<()> {
    let decoder = decoder();
    let restore_fail = decoder.restore_fail.clone();
    let mut runtime = Runtime::new(decoder, LIMITS)?;
    let cancel = Cancellation::default();
    runtime.generate(&[1], &options(), &cancel, |_| Ok(()))?;
    let committed = runtime.tokens().to_vec();
    restore_fail.set(true);
    assert!(
        runtime
            .generate(&[2], &options(), &cancel, |_| {
                Err(std::io::Error::other("disconnected consumer").into())
            })
            .is_err()
    );
    assert!(runtime.is_faulted());
    // A failed backend restoration closes admission, but the public token
    // history must still describe the last successful transaction.
    assert_eq!(runtime.tokens(), committed);
    assert!(
        runtime
            .generate(&[1], &options(), &cancel, |_| Ok(()))
            .is_err()
    );
    assert_eq!(runtime.tokens(), committed);
    runtime.reset()?;
    assert_eq!(runtime.tokens(), &[0_u32; 0]);
    restore_fail.set(false);
    runtime.generate(&[3], &options(), &cancel, |_| Ok(()))?;
    assert_eq!(runtime.tokens(), &[3, 3]);
    Ok(())
}

#[test]
fn bounds_stop_tokens_and_ties_do_not_silently_change_decoding() -> Result<()> {
    assert_eq!(greedy(&[1.0, 2.0, 2.0])?, 1);
    assert!(greedy(&[f32::NAN]).is_err());
    let decoder = decoder();
    let steps = decoder.steps.clone();
    let mut runtime = Runtime::new(decoder, LIMITS)?;
    let cancel = Cancellation::default();
    assert!(
        runtime
            .generate(&[8], &options(), &cancel, |_| Ok(()))
            .is_err()
    );
    assert!(
        runtime
            .generate(&[1; 32], &options(), &cancel, |_| Ok(()))
            .is_err()
    );
    assert_eq!(steps.get(), 0);
    let stop = Generation {
        max_tokens: 4,
        stop_tokens: vec![1],
    };
    let done = runtime.generate(&[1], &stop, &cancel, |_| {
        Err(std::io::Error::other("stop token must not stream").into())
    })?;
    assert_eq!(done.finish, Finish::Stop);
    assert_eq!(done.generated_tokens, 0);
    assert_eq!(runtime.tokens(), &[1]);
    Ok(())
}

#[test]
fn greedy_validates_the_entire_vocabulary_and_preserves_first_ties() -> Result<()> {
    assert!(greedy(&[]).is_err());
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for position in 0..4 {
            let mut logits = [f32::MAX, -1.0, -2.0, -3.0];
            logits[position] = invalid;
            assert!(greedy(&logits).is_err());
        }
    }
    assert_eq!(greedy(&[-0.0, 0.0, -0.0])?, 0);
    assert_eq!(greedy(&[-3.0, -1.0, -1.0, -2.0])?, 1);
    assert_eq!(greedy(&[-2.0, -1.0, 0.0, 1.0])?, 3);
    Ok(())
}

#[test]
fn snapshot_budget_rejects_before_mutating_a_committed_conversation() -> Result<()> {
    let decoder = decoder();
    let steps = decoder.steps.clone();
    let mut runtime = Runtime::new(
        decoder,
        Limits {
            checkpoint_bytes: 1,
            ..LIMITS
        },
    )?;
    assert!(
        runtime
            .generate(&[1], &options(), &Cancellation::default(), |_| Ok(()))
            .is_err()
    );
    assert_eq!(steps.get(), 0);
    assert_eq!(runtime.tokens(), &[0_u32; 0]);
    Ok(())
}

#[derive(Clone, Copy, Default)]
enum OutputFault {
    #[default]
    None,
    PartialFailure,
    WrongSize,
    Nonfinite,
    Cancel,
}

struct Reusing {
    inner: Recurrent,
    storage: Rc<RefCell<Vec<(usize, usize)>>>,
    owned_calls: Rc<Cell<usize>>,
    fault: Rc<Cell<OutputFault>>,
    cancellation: Cancellation,
}

impl Decoder for Reusing {
    type Checkpoint = Vec<u32>;

    fn identity(&self) -> &str {
        self.inner.identity()
    }
    fn vocab_size(&self) -> usize {
        self.inner.vocab_size()
    }
    fn checkpoint_bytes(&self) -> usize {
        self.inner.checkpoint_bytes()
    }
    fn reset(&mut self) -> Result<()> {
        self.inner.reset()
    }
    fn checkpoint(&self) -> Result<Self::Checkpoint> {
        self.inner.checkpoint()
    }
    fn restore(&mut self, checkpoint: &Self::Checkpoint) -> Result<()> {
        self.inner.restore(checkpoint)
    }
    fn evaluate(&mut self, token: u32) -> Result<Vec<f32>> {
        self.owned_calls.set(self.owned_calls.get() + 1);
        let mut output = Vec::new();
        self.evaluate_into(token, &mut output)?;
        Ok(output)
    }
    fn evaluate_into(&mut self, token: u32, output: &mut Vec<f32>) -> Result<()> {
        self.inner.tokens.push(token);
        output.clear();
        output.try_reserve_exact(8)?;
        output.resize(8, -1.0);
        let next = usize::try_from(self.inner.tokens.iter().sum::<u32>() % 8)?;
        output[next] = 1.0;
        self.storage
            .borrow_mut()
            .push((output.as_ptr().addr(), output.capacity()));
        // Deliberately violate the output contract to verify the runtime's
        // defensive validation and rollback independently of the decoder.
        match self.fault.replace(OutputFault::None) {
            OutputFault::None => {}
            OutputFault::PartialFailure => {
                output.truncate(1);
                return Err(std::io::Error::other("partial output after mutation").into());
            }
            OutputFault::WrongSize => {
                output.pop();
            }
            OutputFault::Nonfinite => {
                output[0] = f32::NAN;
            }
            OutputFault::Cancel => self.cancellation.cancel(),
        }
        Ok(())
    }
}

#[test]
fn reusable_logits_preserve_storage_prefixes_and_transaction_recovery() -> Result<()> {
    let storage = Rc::new(RefCell::new(Vec::new()));
    let owned_calls = Rc::new(Cell::new(0));
    let fault = Rc::new(Cell::new(OutputFault::None));
    let cancellation = Cancellation::default();
    let mut runtime = Runtime::new(
        Reusing {
            inner: decoder(),
            storage: storage.clone(),
            owned_calls: owned_calls.clone(),
            fault: fault.clone(),
            cancellation: cancellation.clone(),
        },
        LIMITS,
    )?;
    let active = Cancellation::default();
    runtime.generate(&[1], &options(), &active, |_| Ok(()))?;
    runtime.generate(&[1, 1, 2], &options(), &active, |_| Ok(()))?;
    let edited = runtime.generate(&[1, 1, 3], &options(), &active, |_| Ok(()))?;
    assert_eq!(edited.reused_tokens, 2);
    assert_eq!(runtime.tokens(), &[1, 1, 3, 5]);
    runtime.reset()?;
    runtime.generate(&[1], &options(), &active, |_| Ok(()))?;
    let first_storage = storage.borrow()[0];
    assert!(storage.borrow().iter().all(|&entry| entry == first_storage));
    assert_eq!(owned_calls.get(), 0);

    for failure in [
        OutputFault::PartialFailure,
        OutputFault::WrongSize,
        OutputFault::Nonfinite,
    ] {
        fault.set(failure);
        assert!(
            runtime
                .generate(&[2], &options(), &active, |_| Ok(()))
                .is_err()
        );
        assert_eq!(runtime.tokens(), &[1, 1]);
        assert!(!runtime.is_faulted());
        // Reusing the exact committed prefix must use its saved logits rather
        // than the partial/invalid buffer written by the failed evaluation.
        let restored = runtime.generate(&[1, 1], &options(), &active, |_| Ok(()))?;
        assert_eq!(restored.reused_tokens, 2);
        assert_eq!(runtime.tokens(), &[1, 1, 2]);
        runtime.reset()?;
        runtime.generate(&[1], &options(), &active, |_| Ok(()))?;
    }
    fault.set(OutputFault::Cancel);
    assert!(
        runtime
            .generate(&[2], &options(), &cancellation, |_| Ok(()))
            .is_err()
    );
    assert_eq!(runtime.tokens(), &[1, 1]);
    assert!(!runtime.is_faulted());
    runtime.generate(&[1, 1], &options(), &active, |_| Ok(()))?;
    assert_eq!(runtime.tokens(), &[1, 1, 2]);
    assert_eq!(owned_calls.get(), 0);
    Ok(())
}

#[test]
fn owned_decoder_fallback_reuses_output_and_clears_failed_evaluations() -> Result<()> {
    let mut decoder = decoder();
    let mut output = Vec::with_capacity(16);
    decoder.evaluate_into(1, &mut output)?;
    let address = output.as_ptr();
    let checkpoint = decoder.checkpoint()?;
    let expected = decoder.evaluate(2)?;
    decoder.restore(&checkpoint)?;
    decoder.evaluate_into(2, &mut output)?;
    assert_eq!(output, expected);
    assert_eq!(output.as_ptr(), address);
    decoder.fail.set(true);
    assert!(decoder.evaluate_into(3, &mut output).is_err());
    assert_eq!(output, [0.0_f32; 0]);
    Ok(())
}
