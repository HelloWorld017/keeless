//! One-active-transaction state shared by COM callbacks.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
};

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
    cancelled: AtomicBool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Busy;

pub struct CeremonyGuard<'a> {
    owner: &'a ActiveCeremony,
    state: Arc<State>,
}

impl ActiveCeremony {
    /// Start a ceremony, failing while another transaction is still active.
    pub fn begin(&self, transaction_id: TransactionId) -> Result<CeremonyGuard<'_>, Busy> {
        let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if active.is_some() {
            return Err(Busy);
        }
        let state = Arc::new(State {
            transaction_id,
            cancelled: AtomicBool::new(false),
        });
        *active = Some(state.clone());
        Ok(CeremonyGuard { owner: self, state })
    }

    /// Signal cancellation only when the platform names the active transaction.
    pub fn cancel(&self, transaction_id: TransactionId) -> bool {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(state) = active.as_ref() else {
            return false;
        };
        if state.transaction_id != transaction_id {
            return false;
        }
        state.cancelled.store(true, Ordering::Release);
        true
    }
}

impl CeremonyGuard<'_> {
    /// Check before and after every blocking ceremony stage.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }
}

impl Drop for CeremonyGuard<'_> {
    fn drop(&mut self) {
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
        let first = active.begin([1; 16]).unwrap();
        assert!(matches!(active.begin([2; 16]), Err(Busy)));
        drop(first);
        assert!(active.begin([2; 16]).is_ok());
    }

    #[test]
    fn cancellation_only_affects_the_matching_transaction() {
        let active = ActiveCeremony::default();
        let ceremony = active.begin([1; 16]).unwrap();
        assert!(!active.cancel([2; 16]));
        assert!(!ceremony.is_cancelled());
        assert!(active.cancel([1; 16]));
        assert!(ceremony.is_cancelled());
    }
}
