//! Recurrent-state ownership regressions independent of a model artifact.

use bonsai_prism_runtime::{
    Decoder, Result,
    runtime::{Cancellation, Finish, Generation, Limits, Runtime, greedy},
};
use std::{cell::Cell, rc::Rc};

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
    restore_fail.set(true);
    assert!(
        runtime
            .generate(&[2], &options(), &cancel, |_| {
                Err(std::io::Error::other("disconnected consumer").into())
            })
            .is_err()
    );
    assert!(runtime.is_faulted());
    assert!(
        runtime
            .generate(&[1], &options(), &cancel, |_| Ok(()))
            .is_err()
    );
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
