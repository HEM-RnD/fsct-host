use super::*;
use anyhow::Error;
use std::sync::Mutex;
use tokio::time::{sleep, Duration};
use uuid::Uuid;
use crate::definitions::FsctStatus;

// ----------------- Helpers for selection testing -----------------
fn fold_best(items: &[PlayerSelectionParams]) -> Option<PlayerSelectionParams> {
    let mut current: Option<PlayerSelectionParams> = None;
    for cand in items {
        if is_better_selection(cand, &current) {
            current = Some(*cand);
        }
    }
    current
}

fn permute_indices_rec(n: usize, current: &mut Vec<usize>, used: &mut Vec<bool>, out: &mut Vec<Vec<usize>>) {
    if current.len() == n {
        out.push(current.clone());
        return;
    }
    for i in 0..n {
        if !used[i] {
            used[i] = true;
            current.push(i);
            permute_indices_rec(n, current, used, out);
            current.pop();
            used[i] = false;
        }
    }
}

fn permute_indices(n: usize) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    let mut used = vec![false; n];
    let mut cur = Vec::with_capacity(n);
    permute_indices_rec(n, &mut cur, &mut used, &mut out);
    out
}

fn selection_is_order_independent(items: &[PlayerSelectionParams]) -> (bool, Option<PlayerSelectionParams>) {
    let base = fold_best(items);
    for perm in permute_indices(items.len()) {
        let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
        let w = fold_best(&permuted);
        if w != base {
            return (false, base);
        }
    }
    (true, base)
}

// Physically sort by repeatedly picking the best remaining (deterministic for tests)
fn sort_by_preference(items: &[PlayerSelectionParams]) -> Vec<PlayerSelectionParams> {
    let mut rest: Vec<PlayerSelectionParams> = items.to_vec();
    let mut out = Vec::with_capacity(rest.len());
    while !rest.is_empty() {
        // find index of the best element
        let mut best_idx = 0;
        let mut best_opt: Option<PlayerSelectionParams> = None;
        for (i, cand) in rest.iter().enumerate() {
            if is_better_selection(cand, &best_opt) {
                best_opt = Some(*cand);
                best_idx = i;
            }
        }
        out.push(rest.remove(best_idx));
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
struct ApplyCall {
    device: ManagedDeviceId,
    state: PlayerState,
}

#[derive(Debug, Clone, PartialEq)]
struct TimelineCall {
    device: ManagedDeviceId,
    timeline: Option<TimelineInfo>,
}

#[derive(Debug, Clone, PartialEq)]
struct TextCall {
    device: ManagedDeviceId,
    text_id: FsctTextMetadata,
    text: Option<String>,
}

struct MockApplier {
    calls: Mutex<Vec<ApplyCall>>, // full applies
    timeline_calls: Mutex<Vec<TimelineCall>>, // partial timeline applies
    text_calls: Mutex<Vec<TextCall>>, // partial text applies
}

impl MockApplier {
    fn new() -> Arc<Self> { Arc::new(Self { calls: Mutex::new(Vec::new()), timeline_calls: Mutex::new(Vec::new()), text_calls: Mutex::new(Vec::new()) }) }
    fn take(&self) -> Vec<ApplyCall> { std::mem::take(&mut self.calls.lock().unwrap()) }
    fn take_timeline(&self) -> Vec<TimelineCall> { std::mem::take(&mut self.timeline_calls.lock().unwrap()) }
    fn take_text(&self) -> Vec<TextCall> { std::mem::take(&mut self.text_calls.lock().unwrap()) }
}

impl PlayerStateApplier for MockApplier {
    fn apply_to_device<'a>(&'a self, device_id: ManagedDeviceId, state: &'a PlayerState)
                           -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(), Error>> + Send + 'a>> {
        let st = state.clone();
        Box::pin(async move {
            let mut guard = self.calls.lock().unwrap();
            let duplicate = guard.iter().any(|c| c.device == device_id && c.state == st);
            if !duplicate {
                #[cfg(test)]
                {
                    println!("APPLY dev={:?} status={:?}", device_id, st.status);
                }
                guard.push(ApplyCall { device: device_id, state: st });
            }
            Ok(())
        })
    }

    fn apply_status<'a>(&'a self, _device_id: ManagedDeviceId, _status: crate::definitions::FsctStatus)
                        -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(), Error>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    fn apply_timeline<'a>(&'a self, device_id: ManagedDeviceId, timeline: Option<crate::definitions::TimelineInfo>)
                          -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(), Error>> + Send + 'a>> {
        Box::pin(async move {
            self.timeline_calls.lock().unwrap().push(TimelineCall { device: device_id, timeline: timeline.clone() });
            Ok(())
        })
    }

    fn apply_text<'a>(&'a self, device_id: ManagedDeviceId, text_id: crate::definitions::FsctTextMetadata, text: Option<&'a str>)
                      -> std::pin::Pin<Box<dyn std::future::Future<Output=Result<(), Error>> + Send + 'a>> {
        let owned = text.map(|s| s.to_string());
        Box::pin(async move {
            self.text_calls.lock().unwrap().push(TextCall { device: device_id, text_id, text: owned });
            Ok(())
        })
    }
}

fn make_ids(n: usize) -> Vec<ManagedDeviceId> { (0..n).map(|_| Uuid::new_v4()).collect() }
fn pid(n: u32) -> ManagedPlayerId { std::num::NonZeroU32::new(n).unwrap() }

fn default_state_with_title(title: &str) -> PlayerState {
    let mut s = PlayerState::default();
    s.texts.get_mut_text(crate::definitions::FsctTextMetadata::CurrentTitle).replace(title.to_string());
    s
}

// Helper to build orchestrator and the senders
fn build_orchestrator(applier: Arc<MockApplier>) -> (
    Orchestrator<MockApplier>,
    tokio::sync::broadcast::Sender<PlayerEvent>,
    tokio::sync::broadcast::Sender<DeviceEvent>,
) {
    let (player_tx, player_rx) = tokio::sync::broadcast::channel(256);
    let (device_tx, device_rx) = tokio::sync::broadcast::channel(256);
    let orch = Orchestrator::new_with_applier(player_rx, device_rx, applier);
    (orch, player_tx, device_tx)
}

async fn run_orchestrator(orch: Orchestrator<MockApplier>) -> ServiceHandle {
    orch.run()
}

async fn short_wait() { sleep(Duration::from_millis(10)).await }

#[tokio::test]
async fn zero_players_zero_devices_no_apply() {
    let applier = MockApplier::new();
    let (orch, _ptx, _dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    short_wait().await;
    assert!(applier.take().is_empty());
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn one_player_zero_devices_state_update_no_apply() {
    let applier = MockApplier::new();
    let (orch, ptx, _dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(1);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1 });

    short_wait().await;
    assert!(applier.take().is_empty());
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn zero_players_one_device_add_no_apply() {
    let applier = MockApplier::new();
    let (orch, _ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    assert!(applier.take().is_empty());
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn unassigned_state_then_device_added_applies_to_device() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(1);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    short_wait().await;
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));

    short_wait().await;
    let calls = applier.take();
    // allow possible initial Unknown applies; require that S1 was applied to d at least once
    assert!(calls.iter().any(|c| c.device == d && c.state == s1));
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn assign_before_connect_then_connect_then_update() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(1);
    let d = make_ids(1)[0];
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
    // give orchestrator a moment to record the assignment before device connects
    short_wait().await;
    // device connects after assignment
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    // should apply s1 once due to device added with assigned player
    let mut calls = applier.take();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].device, d);
    assert_eq!(calls[0].state, s1);

    // update to S2 -> apply again to assigned device
    let s2 = default_state_with_title("S2");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s2.clone() });
    short_wait().await;
    calls = applier.take();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].device, d);
    assert_eq!(calls[0].state, s2);

    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn multiple_players_one_device_unassigned_and_assignment_switch() {
    env_logger::init();
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let d = make_ids(1)[0];
    let p1 = pid(1);
    let p2 = pid(2);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    short_wait().await;

    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    short_wait().await;
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let mut calls = applier.take();
    // Accept possible initial Unknown applies; ensure S1 reached device d
    assert!(calls.iter().any(|c| c.device == d && c.state == s1));

    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
    short_wait().await;

    calls = applier.take();
    // ensure S2 did not reach device d yet
    assert!(calls.is_empty());

    // P2 updates -> becomes not selected; should not propagate to unassigned device d
    let mut s2 = default_state_with_title("S2");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    calls = applier.take();
    // ensures S2 did not reach device d yet
    assert!(calls.is_empty());

    // Now assign P2 to d -> still nothing has changed, since both player are not playing
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p2, device_id: d });
    short_wait().await;
    calls = applier.take();

    // ensures S2 did not reach device d yet
    assert!(calls.is_empty());

    // P2 updates -> becomes playing; should propagate to assigned device d
    s2.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    calls = applier.take();

    // P2 has known state s2 and device connected, assignment applies s2 (at least once)
    assert!(calls.iter().any(|c| c.device == d && c.state == s2));

    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn one_player_multiple_devices_unassigned_then_assign() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let p1 = pid(1);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    short_wait().await;
    let ids = make_ids(2);
    let d1 = ids[0];
    let d2 = ids[1];
    let _ = dtx.send(DeviceEvent::Added(d1));
    let _ = dtx.send(DeviceEvent::Added(d2));
    short_wait().await;
    let mut calls = applier.take();
    // both devices should eventually receive s1 (there may be initial Unknown applies)
    assert!(calls.iter().any(|c| c.device == d1 && c.state == s1));
    assert!(calls.iter().any(|c| c.device == d2 && c.state == s1));

    // Assign player to d1 -> should apply s1 to d1 again; d2 remains unassigned with prior state
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d1 });
    short_wait().await;
    calls = applier.take();
    // S1 has not changed, so nothing has been applied to d1
    assert!(!calls.iter().any(|c| c.device == d1));
    // But now d2 is not assigned to any player, so the default state should be applied to d2
    assert!(calls.iter().any(|c| c.device == d2 && c.state == PlayerState::default()));

    // Update to S2 -> applies to assigned device d1
    let s2 = default_state_with_title("S2");
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s2.clone() });
    short_wait().await;
    calls = applier.take();
    assert!(calls.iter().any(|c| c.device == d1 && c.state == s2));

    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn general_group_picks_playing() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let p1 = pid(1);
    let p2 = pid(2);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
    let mut s1 = default_state_with_title("S1");
    s1.status = FsctStatus::Playing;
    let mut s2 = default_state_with_title("S2");
    s2.status = FsctStatus::Paused;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let calls = applier.take();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].state, s1);
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn multiple_playing_keep_last_active_in_general() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let p1 = pid(1);
    let p2 = pid(2);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
    let mut s1 = default_state_with_title("S1");
    s1.status = FsctStatus::Playing;
    let mut s2 = default_state_with_title("S2");
    s2.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let _ = applier.take(); // p1 selected
    // now p2 starts playing as well
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    let calls = applier.take();
    // ambiguous, should keep last active (p1) and not reapply since state didn't change for p1
    assert!(calls.is_empty());
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn device_group_with_multiple_players_picks_playing() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    let p1 = pid(1);
    let p2 = pid(2);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
    let mut s1 = default_state_with_title("S1");
    s1.status = FsctStatus::Paused;
    let mut s2 = default_state_with_title("S2");
    s2.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d });
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p2, device_id: d });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    let calls = applier.take();
    assert!(!calls.is_empty());
    assert_eq!(calls.last().unwrap().device, d);
    assert_eq!(calls.last().unwrap().state, s2);
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn assigned_to_disconnected_counts_as_general() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let d_assigned = make_ids(1)[0]; // will remain disconnected
    let d_unassigned = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d_unassigned));
    let p1 = pid(1);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let s1 = default_state_with_title("S1");
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d_assigned });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    short_wait().await;
    let calls = applier.take();
    // p1 should be applied to unassigned connected device (there may be an initial Unknown)
    assert!(calls.iter().any(|c| c.device == d_unassigned && c.state == s1));
    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn general_does_not_pick_playing_assigned_to_other_device() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;
    let p1 = pid(1);
    let p2 = pid(2);
    let d1 = make_ids(1)[0];
    let d2 = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d1)); // device with assigned group
    let _ = dtx.send(DeviceEvent::Added(d2)); // unassigned mirrors general
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p1".into() });
    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p2".into() });
    let mut s1 = default_state_with_title("S1");
    s1.status = FsctStatus::Playing;
    let mut s2 = default_state_with_title("S2");
    s2.status = FsctStatus::Paused;
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p1, device_id: d1 });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p2, state: s2.clone() });
    short_wait().await;
    let calls = applier.take();
    // d1 gets s1 due to assignment; general (unassigned) should prefer unassigned p2 over playing p1 assigned elsewhere
    assert!(calls.iter().any(|c| c.device == d1 && c.state == s1));
    assert!(calls.iter().any(|c| c.device == d2 && c.state == s2));
    let _ = handle.shutdown().await;
}

#[test]
fn is_better_selection_order_independence_three_cases() {
    // Build three elements as requested:
    // 1) playing unassigned
    // 2) non-playing
    // 3) non-playing assigned to current device
    let a_playing_unassigned = PlayerSelectionParams {
        status: PlaybackStatus::Playing,
        assignment: Assignment::Unassigned,
        is_last_selected: false,
    };
    let b_last_selected_and_playing = PlayerSelectionParams {
        status: PlaybackStatus::Playing,
        assignment: Assignment::Unassigned,
        is_last_selected: true,
    };
    let c_non_playing_assigned_here = PlayerSelectionParams {
        status: PlaybackStatus::Stopped,
        assignment: Assignment::AssignedToThisDevice,
        is_last_selected: false,
    };

    let items = vec![
        a_playing_unassigned,
        b_last_selected_and_playing,
        c_non_playing_assigned_here,
    ];

    // Use helper to verify order-independence and assert expected winner
    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable, "Winner should be identical across all permutations");
    assert_eq!(winner, Some(b_last_selected_and_playing), "Last selected and playing should beat playing unassigned and idle assigned-here in this triad");

    // Additionally, verify sorting stability across all permutations using the helper sort
    let baseline_sorted = sort_by_preference(&items);
    for perm in permute_indices(items.len()) {
        let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
        let sorted = sort_by_preference(&permuted);
        assert_eq!(sorted, baseline_sorted, "Sorting should be stable regardless of input order for the 3-case scenario");
    }
}

#[test]
fn is_better_selection_order_independence_six_players_and_sort_stability() {
    let p_a_playing_assigned_here = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let p_b_user_selected_idle = PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::Unassigned, is_last_selected: false };
    let p_c_playing_unassigned = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false };
    let p_d_playing_assigned_other = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
    let p_e_idle_assigned_here = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let p_f_idle_unassigned_last = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: true };

    let items = vec![
        p_a_playing_assigned_here,
        p_b_user_selected_idle,
        p_c_playing_unassigned,
        p_d_playing_assigned_other,
        p_e_idle_assigned_here,
        p_f_idle_unassigned_last,
    ];

    // Check order independence of the winner
    let (stable, base_winner) = selection_is_order_independent(&items);
    assert!(stable, "Winner should be the same for all permutations");
    assert_eq!(base_winner, Some(p_a_playing_assigned_here), "Expected the strongest candidate to win");

    // Check that the full sorting is stable across permutations (deterministic for this set)
    let baseline_sorted = sort_by_preference(&items);
    for perm in permute_indices(items.len()) {
        let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
        let sorted = sort_by_preference(&permuted);
        assert_eq!(sorted, baseline_sorted, "Sorting should be stable regardless of input order");
    }
}

#[test]
fn is_better_selection_tie_broken_by_last_selected() {
    // All identical except is_last_selected
    let x1 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false };
    let x2 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: true }; // should win
    let x3 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false };
    let x4 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false };
    let items = vec![x1, x2, x3, x4];

    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable, "Tie breaker by last selected must be order independent");
    assert_eq!(winner, Some(x2), "The one flagged as last selected should be preferred among equals");
}

#[test]
fn is_better_selection_penalizes_assigned_to_other_device() {
    // Playing but assigned elsewhere should lose to an idle unassigned
    let playing_other = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
    let idle_unassigned = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false };
    let items = vec![playing_other, idle_unassigned];

    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable);
    assert_eq!(winner, Some(idle_unassigned), "Idle unassigned should be preferred over playing assigned to other device");
}

#[test]
fn is_better_selection_both_playing_assignment_order() {
    // Verify assignment precedence when both are playing:
    // AssignedToThisDevice > UserSelected > Unassigned > AssignedToOtherDevice
    let playing_here = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let playing_unassigned = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false };
    let playing_other = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };

    // Pairwise checks via order-independence helper
    let cases = vec![
        (vec![playing_here, playing_unassigned], playing_here),
        (vec![playing_here, playing_other], playing_here),
        (vec![playing_unassigned, playing_other], playing_unassigned),
    ];
    for (items, expected) in cases {
        let (stable, winner) = selection_is_order_independent(&items);
        assert!(stable, "Winner should be order independent for pairwise playing comparison");
        assert_eq!(winner, Some(expected));
    }
}

#[test]
fn is_better_selection_playing_unassigned_beats_idle_assigned_here() {
    // No special-case should override generic rule that playing beats non-playing
    let playing_unassigned = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false };
    let idle_here = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let items = vec![idle_here, playing_unassigned];
    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable);
    assert_eq!(winner, Some(playing_unassigned), "Playing unassigned should beat idle assigned-here");
}

#[test]
fn is_better_selection_last_selected_breaks_tie_when_both_playing_same_assignment() {
    // Identical state except last_selected, both playing and unassigned
    let a = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false };
    let b = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: true };
    let items = vec![a, b];
    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable);
    assert_eq!(winner, Some(b), "Last selected should win among identical playing candidates");
}

#[test]
fn is_better_selection_four_players_permutation_and_sort() {
    // A nuanced set to test full permutation stability and deterministic sorting
    // Compose so that final order (best to worst) should be:
    // 1) playing assigned here, 2) playing user-selected, 3) idle user-selected, 4) playing assigned to other
    let p1 = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let p2 = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::Unassigned, is_last_selected: false };
    let p3 = PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToThisDevice, is_last_selected: false };
    let p4 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::Unassigned, is_last_selected: false };
    let items = vec![p1, p2, p3, p4];

    // Winner must be p1 for all permutations
    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable);
    assert_eq!(winner, Some(p1));

    // Sorting stability across all permutations
    let baseline_sorted = sort_by_preference(&items);
    // Confirm expected sorting shape quickly
    assert_eq!(baseline_sorted[0], p1);
    assert_eq!(baseline_sorted[1], p2);
    // Positions 3 and 4 follow rules: p3 (idle user-selected) should beat p4 (playing assigned to other) due to special-case penalty
    assert_eq!(baseline_sorted[2], p3);
    assert_eq!(baseline_sorted[3], p4);

    for perm in permute_indices(items.len()) {
        let permuted: Vec<PlayerSelectionParams> = perm.iter().map(|&i| items[i]).collect();
        let sorted = sort_by_preference(&permuted);
        assert_eq!(sorted, baseline_sorted, "Sorting should be stable for the 4-player nuanced set");
    }
}

#[test]
fn is_better_selection_all_assigned_to_other_device_picks_nothing() {
    // All candidates are AssignedToOtherDevice; playing should win even if an idle one was last selected
    let playing_other = PlayerSelectionParams { status: PlaybackStatus::Playing, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
    let idle_other_1 = PlayerSelectionParams { status: PlaybackStatus::Paused, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
    let idle_other_2_last = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToOtherDevice, is_last_selected: true };
    let idle_other_3 = PlayerSelectionParams { status: PlaybackStatus::Stopped, assignment: Assignment::AssignedToOtherDevice, is_last_selected: false };
    let items = vec![idle_other_1, playing_other, idle_other_2_last, idle_other_3];

    let (stable, winner) = selection_is_order_independent(&items);
    assert!(stable);
    assert_eq!(winner, None, "Among candidates all assigned to other devices, none should win");
}

#[tokio::test]
async fn timeline_update_triggers_partial_apply_only() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(101);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p101".into() });
    let mut s1 = default_state_with_title("Initial");
    s1.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let _ = applier.take(); // clear initial full apply(s)

    // Send timeline update
    let tl = TimelineInfo {
        position: std::time::Duration::from_secs(12),
        update_time: std::time::SystemTime::now(),
        duration: std::time::Duration::from_secs(300),
        rate: 1.0,
    };
    let _ = ptx.send(PlayerEvent::TimelineUpdated { player_id: p1, timeline: tl.clone() });
    short_wait().await;

    // Expect only partial timeline calls, no full apply
    let full_calls = applier.take();
    let tl_calls = applier.take_timeline();
    assert!(full_calls.is_empty(), "Timeline update should not trigger full apply_to_device");
    assert_eq!(tl_calls.len(), 1, "Expected exactly one timeline partial apply");
    assert_eq!(tl_calls[0].device, d);
    assert_eq!(tl_calls[0].timeline, Some(tl));

    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn text_update_triggers_partial_apply_only() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(102);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p102".into() });
    let mut s1 = default_state_with_title("Initial");
    s1.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });
    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let _ = applier.take(); // clear initial full apply(s)

    // Send text metadata update
    let new_title = "New Title".to_string();
    let _ = ptx.send(PlayerEvent::TextMetadataUpdated { player_id: p1, metadata: FsctTextMetadata::CurrentTitle, text: Some(new_title.clone()) });
    short_wait().await;

    let full_calls = applier.take();
    let txt_calls = applier.take_text();
    assert!(full_calls.is_empty(), "Text update should not trigger full apply_to_device");
    assert_eq!(txt_calls.len(), 1, "Expected exactly one text partial apply");
    assert_eq!(txt_calls[0].device, d);
    assert_eq!(txt_calls[0].text_id, FsctTextMetadata::CurrentTitle);
    assert_eq!(txt_calls[0].text, Some(new_title));

    let _ = handle.shutdown().await;
}

#[tokio::test]
async fn status_update_reassigns_and_full_apply() {
    let applier = MockApplier::new();
    let (orch, ptx, dtx) = build_orchestrator(applier.clone());
    let handle = run_orchestrator(orch).await;

    let p1 = pid(201);
    let p2 = pid(202);
    let _ = ptx.send(PlayerEvent::Registered { player_id: p1, self_id: "p201".into() });
    let _ = ptx.send(PlayerEvent::Registered { player_id: p2, self_id: "p202".into() });

    let mut s1 = default_state_with_title("P1");
    s1.status = FsctStatus::Playing;
    let _ = ptx.send(PlayerEvent::StateUpdated { player_id: p1, state: s1.clone() });

    let d = make_ids(1)[0];
    let _ = dtx.send(DeviceEvent::Added(d));
    short_wait().await;
    let _ = applier.take(); // p1 applied due to selection

    // Assign p2 to this device, but no state yet; ensure no apply happens
    let _ = ptx.send(PlayerEvent::Assigned { player_id: p2, device_id: d });
    short_wait().await;
    assert!(applier.take().is_empty());

    // Now status update on p2 should cause selection to switch and trigger full apply
    let _ = ptx.send(PlayerEvent::StatusUpdated { player_id: p2, status: FsctStatus::Playing });
    short_wait().await;
    let calls = applier.take();
    assert_eq!(calls.len(), 1, "Expected one full apply after status change causing reassignment");
    assert_eq!(calls[0].device, d);
    assert_eq!(calls[0].state.status, FsctStatus::Playing);
    // And no partials recorded for this scenario
    assert!(applier.take_timeline().is_empty());
    assert!(applier.take_text().is_empty());

    let _ = handle.shutdown().await;
}

#[test]
fn scoring_order_test()
{
    let mut last_score = -1;
    for player_selection_params in super::PLAYER_SELECTION_PARAMS_ALL_COMBINATIONS.iter() {
        let score = player_selection_params.score();
        println!("SCORE: {:2} | {:?}", score, player_selection_params);
        assert!(score > last_score || (score == 0 && last_score == 0), "Scores should be increasing, or zero in the begining");
        last_score = score;
    }
}