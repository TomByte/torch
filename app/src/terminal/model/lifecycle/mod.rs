//! Coordinates terminal block lifecycle transitions.
//!
//! `TerminalModel` supplies lifecycle inputs together with a snapshot of live block state. The
//! coordinator reconciles its remembered phase against that snapshot and asks the pure transition
//! policy for an action. Callers apply the planned action before committing its next phase.

mod transition;

use torch_core::features::FeatureFlag;
pub(in crate::terminal) use transition::{
    CommandStartKind, IgnoreReason, LifecycleAction, LifecycleInput, LifecyclePhase,
    LifecycleSnapshot, LifecycleTransition, NextBlockIdDisposition, PreexecObservation,
};

/// Describes whether a command-start intent was accepted or conservatively ignored.
///
/// Callers must perform start-dependent side effects, such as writing PTY bytes or attaching
/// metadata only when this outcome is [`StartCommandOutcome::Accepted`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartCommandOutcome {
    /// The active block was started.
    Accepted,
    /// The active block was already submitted, so the repeated start was coalesced.
    Coalesced,
    /// The active block was executing, so starting another command was rejected.
    RejectedExecuting,
    /// The terminal had terminated, so the start intent was ignored.
    IgnoredTerminated,
}

impl StartCommandOutcome {
    /// Returns whether callers may perform side effects associated with starting the command.
    pub fn is_accepted(self) -> bool {
        matches!(self, StartCommandOutcome::Accepted)
    }
}

/// Remembers the lifecycle phase for one terminal and plans transitions without applying mutations.
///
/// The coordinator deliberately does not own `TerminalModel` or `BlockList`. This keeps transition
/// policy testable as pure data while requiring the model to apply an accepted action before it
/// commits the planned phase.
pub(super) struct BlockLifecycleCoordinator {
    phase: LifecyclePhase,
}

impl Default for BlockLifecycleCoordinator {
    fn default() -> Self {
        Self {
            phase: LifecyclePhase::Unknown,
        }
    }
}

impl BlockLifecycleCoordinator {
    /// Plans the action and next phase for an observed lifecycle input.
    ///
    /// Planning first reconciles the remembered phase with live block evidence so stale internal
    /// state cannot authorize a mutation. It then evaluates the pure transition policy. This method
    /// does not advance the remembered phase; the caller must apply the returned action and then
    /// call [`Self::commit`].
    pub(super) fn plan(
        &mut self,
        snapshot: &LifecycleSnapshot,
        input: LifecycleInput,
    ) -> LifecycleTransition {
        let previous_phase = transition::reconcile_phase(self.phase, snapshot);
        let (planned_next_phase, planned_action) = transition::plan(previous_phase, input);
        let recovers_command_finished = matches!(
            (input, planned_action),
            (
                LifecycleInput::CommandFinished(NextBlockIdDisposition::Novel),
                LifecycleAction::AcceptCommandFinished,
            )
        ) && match previous_phase {
            LifecyclePhase::AwaitingPrecmd | LifecyclePhase::Unknown => true,
            LifecyclePhase::AtPrompt => snapshot.is_bootstrap_done,
            LifecyclePhase::Submitted | LifecyclePhase::Executing | LifecyclePhase::Terminated => {
                false
            }
        };
        let is_gated_recovery = recovers_command_finished
            || matches!(
                planned_action,
                LifecycleAction::ReconcileCompletionThenApplyPrecmd
            );
        let (next_phase, action) =
            if is_gated_recovery && !FeatureFlag::TerminalLifecycleRecovery.is_enabled() {
                (
                    previous_phase,
                    LifecycleAction::Ignore(IgnoreReason::RecoveryDisabled),
                )
            } else {
                (planned_next_phase, planned_action)
            };
        LifecycleTransition {
            previous_phase,
            next_phase,
            action,
        }
    }

    /// Commits a transition after its planned action has been applied, replacing the coordinator's
    /// remembered phase with its planned next phase.
    pub(super) fn commit(&mut self, transition: &LifecycleTransition) {
        self.phase = transition.next_phase;
    }

    /// Forgets the remembered phase, as if block state had been supplied from outside the
    /// lifecycle inputs.
    #[cfg(test)]
    pub(super) fn reset_unknown(&mut self) {
        self.phase = LifecyclePhase::Unknown;
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
