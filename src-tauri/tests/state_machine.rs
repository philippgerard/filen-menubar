//! Integration tests for the state machine
//!
//! These tests verify the AppState behavior with the watch channel
//! for reactive updates.

mod common;

use common::{wait_for_state, TestEnvironment};
use filen_menubar_lib::state::SyncState;

#[tokio::test]
async fn test_state_transitions_notify_subscribers() {
    let env = TestEnvironment::new();
    let mut rx = env.app_state.subscribe();

    // Transition to Scanning
    env.app_state.set_sync_state(SyncState::Scanning).await;

    // Should receive notification
    assert!(rx.changed().await.is_ok());
    assert_eq!(rx.borrow().sync_state, SyncState::Scanning);
}

#[tokio::test]
async fn test_state_transitions_through_sync_cycle() {
    let env = TestEnvironment::new();

    // Simulate a typical sync cycle
    // Starting -> Scanning -> Syncing -> Synced

    env.app_state.set_sync_state(SyncState::Scanning).await;
    assert!(wait_for_state(&env.app_state, SyncState::Scanning, 100).await);

    env.app_state.set_sync_state(SyncState::Syncing).await;
    assert!(wait_for_state(&env.app_state, SyncState::Syncing, 100).await);

    env.app_state.set_sync_state(SyncState::Synced).await;
    assert!(wait_for_state(&env.app_state, SyncState::Synced, 100).await);
}

#[tokio::test]
async fn test_pending_count_updates() {
    let env = TestEnvironment::new();
    let mut rx = env.app_state.subscribe();

    // Set pending count
    env.app_state.set_pending_count(5).await;

    // Should receive notification
    assert!(rx.changed().await.is_ok());
    assert_eq!(rx.borrow().pending_count, 5);

    // Update pending count
    env.app_state.set_pending_count(3).await;

    assert!(rx.changed().await.is_ok());
    assert_eq!(rx.borrow().pending_count, 3);
}

#[tokio::test]
async fn test_multiple_subscribers_receive_updates() {
    let env = TestEnvironment::new();
    let mut rx1 = env.app_state.subscribe();
    let mut rx2 = env.app_state.subscribe();
    let mut rx3 = env.app_state.subscribe();

    // Make a state change
    env.app_state.set_sync_state(SyncState::Synced).await;

    // All subscribers should be notified
    assert!(rx1.changed().await.is_ok());
    assert!(rx2.changed().await.is_ok());
    assert!(rx3.changed().await.is_ok());

    // All should have the same state
    assert_eq!(rx1.borrow().sync_state, SyncState::Synced);
    assert_eq!(rx2.borrow().sync_state, SyncState::Synced);
    assert_eq!(rx3.borrow().sync_state, SyncState::Synced);
}

#[tokio::test]
async fn test_offline_to_scanning_transition() {
    let env = TestEnvironment::new();

    // Set to Scanning first (valid from Starting)
    env.app_state.set_sync_state(SyncState::Scanning).await;

    // Then to Offline (valid from Scanning)
    env.app_state.set_sync_state(SyncState::Offline).await;
    assert!(wait_for_state(&env.app_state, SyncState::Offline, 100).await);

    // Then retry by going back to Scanning
    env.app_state.set_sync_state(SyncState::Scanning).await;
    assert!(wait_for_state(&env.app_state, SyncState::Scanning, 100).await);
}
