//! One-active-transaction state shared by COM callbacks.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU8, Ordering},
};

use tokio::sync::watch;

/// Opaque Windows WebAuthn transaction identifier.
pub type TransactionId = [u8; 16];

/// Rejects concurrent ceremonies and lets cancellation target only its own
/// transaction. A dropped guard clears its entry even when a callback fails.
#[derive(Default)]
pub struct ActiveCeremony {
    active: Mutex<Option<Arc<State>>>,
}

struct State {
    transaction_id: TransactionId,
    encoded_request: Arc<[u8]>,
    // `0` is active, `1` is cancelled, and `2` is complete. A single compare-
    // exchange decides whether cancellation or response delivery wins.
    phase: AtomicU8,
    cancellation: watch::Sender<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Busy;

pub struct CeremonyGuard<'a> {
    owner: &'a ActiveCeremony,
    state: Arc<State>,
}

/// A signature-verified cancellation may only signal the specific ceremony it
/// observed. Holding this target never affects a later ceremony, even if a
/// platform reuses a transaction identifier.
pub struct CancellationTarget {
    state: Arc<State>,
}

impl ActiveCeremony {
    pub fn is_active(&self) -> bool {
        self.active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Start a ceremony, failing while another transaction is still active.
    pub fn begin(
        &self,
        transaction_id: TransactionId,
        encoded_request: impl Into<Arc<[u8]>>,
    ) -> Result<CeremonyGuard<'_>, Busy> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.is_some() {
            return Err(Busy);
        }
        let (cancellation, _) = watch::channel(false);
        let state = Arc::new(State {
            transaction_id,
            encoded_request: encoded_request.into(),
            phase: AtomicU8::new(0),
            cancellation,
        });
        *active = Some(state.clone());
        Ok(CeremonyGuard { owner: self, state })
    }

    /// Return the original request bytes for signature verification before
    /// cancellation is signalled.
    pub fn cancellation_target(&self, transaction_id: TransactionId) -> Option<CancellationTarget> {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(state) = active.as_ref() else {
            return None;
        };
        if state.transaction_id != transaction_id {
            return None;
        }
        Some(CancellationTarget {
            state: state.clone(),
        })
    }
}

impl CeremonyGuard<'_> {
    /// Check before and after every blocking ceremony stage.
    pub fn is_cancelled(&self) -> bool {
        self.state.phase.load(Ordering::Acquire) == 1
    }

    /// Wait until cancellation wins the active ceremony race.
    pub async fn cancelled(&self) {
        let mut receiver = self.state.cancellation.subscribe();
        if *receiver.borrow() {
            return;
        }
        let _ = receiver.changed().await;
    }

    /// Reserve response delivery. `false` means a cancellation already won and
    /// the caller must not return a late response to Windows.
    pub fn complete(&self) -> bool {
        self.state
            .phase
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

impl CancellationTarget {
    pub fn encoded_request(&self) -> &[u8] {
        &self.state.encoded_request
    }

    /// Signal the ceremony only after the caller verifies the cancellation
    /// signature against [`Self::encoded_request`].
    pub fn cancel(self) -> bool {
        if self
            .state
            .phase
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        self.state.cancellation.send_replace(true);
        true
    }
}

impl Drop for CeremonyGuard<'_> {
    fn drop(&mut self) {
        let _ = self
            .state
            .phase
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
        let mut active = self
            .owner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if active
            .as_ref()
            .is_some_and(|state| Arc::ptr_eq(state, &self.state))
        {
            *active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overlap_and_releases_when_the_callback_returns() {
        let active = ActiveCeremony::default();
        assert!(!active.is_active());
        let first = active.begin([1; 16], [1]).unwrap();
        assert!(active.is_active());
        assert!(matches!(active.begin([2; 16], [2]), Err(Busy)));
        drop(first);
        assert!(!active.is_active());
        assert!(active.begin([2; 16], [2]).is_ok());
    }

    #[test]
    fn cancellation_only_affects_the_matching_transaction() {
        let active = ActiveCeremony::default();
        let ceremony = active.begin([1; 16], [4, 5]).unwrap();
        assert!(active.cancellation_target([2; 16]).is_none());
        assert!(!ceremony.is_cancelled());
        let target = active.cancellation_target([1; 16]).unwrap();
        assert_eq!(target.encoded_request(), [4, 5]);
        assert!(target.cancel());
        assert!(ceremony.is_cancelled());
    }

    #[test]
    fn cancellation_prevents_a_late_response() {
        let active = ActiveCeremony::default();
        let ceremony = active.begin([1; 16], []).unwrap();
        assert!(active.cancellation_target([1; 16]).unwrap().cancel());
        assert!(!ceremony.complete());
    }

    #[test]
    fn completion_prevents_a_late_cancellation() {
        let active = ActiveCeremony::default();
        let ceremony = active.begin([1; 16], []).unwrap();
        let target = active.cancellation_target([1; 16]).unwrap();
        assert!(ceremony.complete());
        assert!(!target.cancel());
    }
}
