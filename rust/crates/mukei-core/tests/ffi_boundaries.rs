//! Integration tests for native boundary safety.
//!
//! Android uses bounded queue polling rather than framework callbacks. These
//! tests verify the safe `BoundaryLease` contract, panic containment, generation
//! invalidation, and process-unique instance identities without raw pointers.

use mukei_core::callback_with_guard;
use mukei_core::guard::{BoundaryLease, GuardError, Inner};

#[test]
fn boundary_lease_allows_work_while_current() {
    let owner = Inner::new();
    let lease = BoundaryLease::capture(&owner);

    let result: Result<i32, GuardError> =
        callback_with_guard!(lease, { Ok::<_, GuardError>(42) });

    assert_eq!(result.unwrap(), 42);
}

#[test]
fn catch_unwind_contains_panic_at_boundary() {
    let owner = Inner::new();
    let lease = BoundaryLease::capture(&owner);

    let result: Result<i32, GuardError> = callback_with_guard!(lease, {
        panic!("simulated panic in native boundary operation");
    });

    assert_eq!(result.unwrap_err(), GuardError::Panic);
}

#[test]
fn tombstoned_owner_is_rejected() {
    let owner = Inner::new();
    let lease = BoundaryLease::capture(&owner);
    owner.tombstone();

    let result: Result<i32, GuardError> =
        callback_with_guard!(lease, { Ok::<_, GuardError>(99) });

    assert_eq!(result.unwrap_err(), GuardError::Released);
}

#[test]
fn bump_allows_rebind_and_rejects_old_lease() {
    let owner = Inner::new();
    let old_lease = BoundaryLease::capture(&owner);
    owner.bump();
    let new_lease = BoundaryLease::capture(&owner);

    let old_result: Result<i32, GuardError> =
        callback_with_guard!(old_lease, { Ok::<_, GuardError>(1) });
    assert!(matches!(
        old_result.unwrap_err(),
        GuardError::GenerationMismatch { .. }
    ));

    let new_result: Result<i32, GuardError> =
        callback_with_guard!(new_lease, { Ok::<_, GuardError>(2) });
    assert_eq!(new_result.unwrap(), 2);
}

#[test]
fn invalid_lease_always_returns_released() {
    let invalid = BoundaryLease::invalid();
    assert!(!invalid.is_valid());

    let result: Result<i32, GuardError> =
        callback_with_guard!(invalid, { Ok::<_, GuardError>(42) });

    assert_eq!(result.unwrap_err(), GuardError::Released);
}

#[test]
fn instance_id_provides_aba_mitigation() {
    let first = Inner::new();
    let second = Inner::new();

    assert_ne!(first.instance_id(), second.instance_id());
    assert_eq!(first.instance_id(), first.instance_id());
}
