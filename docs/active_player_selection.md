# FSCT Active Player Selection: Current Specification (Authoritative) and Historical Proposal

Status: Authoritative (implementation-aligned). This document describes the current behavior and architecture for selecting the “active player” for device groups in FSCT, aligned with the existing orchestrator implementation.

Important: There is no global fallback to “any playing”; unassigned devices do not borrow states from other device groups (see section 11).

Warning: Sections 1–10 below are retained as a historical proposal and are not authoritative. If any text below contradicts Section 0 or the code in core/src/orchestrator.rs, Section 0 prevails.

## 0. Current Implementation Selection Algorithm (Authoritative)

This section describes exactly how the current orchestrator chooses a player for each connected device, based on the code in core/src/orchestrator.rs. Where this section contradicts later, more speculative sections, this one prevails.

Key points:
- There is no separate "general group" selection memory. Selection is computed independently for each connected device.
- For each device, the orchestrator scans all known players and evaluates them with a comparator. The best-scoring player becomes the device's active player.
- Assignment ordering depends on playing status:
  - Both candidates are Playing → use assignment order: AssignedToThisDevice > UserSelected > Unassigned > AssignedToOtherDevice.
  - Both candidates are Non-Playing → UserSelected is promoted above all others (it beats even AssignedToThisDevice). AssignedToOtherDevice remains worst.
- Tie-breaking and playing status rules:
  - If two candidates have the same assignment category and the same playing status, prefer the one that was last selected on this device (stability within the device).
  - If assignment categories are the same, a Playing candidate beats a non-Playing one.
  - When playing status differs between candidates:
    - A Playing Unassigned does NOT beat a non-Playing UserSelected (preferred) candidate.
    - A Playing AssignedToOtherDevice never wins over other categories (deprioritized even against non-playing others).
    - Otherwise, Playing generally wins (e.g., a Playing UserSelected beats an idle AssignedToThisDevice).
  - Among non-Playing candidates, anything beats AssignedToOtherDevice, and UserSelected is the strongest.
- A player assigned to a disconnected device is treated as Unassigned for the purpose of selection (they are not considered "assigned to other device" until that device is connected).

Pseudocode (mirrors the implementation):
```
# inputs per player: player.state.status, player.assigned_device, player.is_assigned_device_attached
# inputs global: preferred_player (Option<PlayerId])
# inputs per device: last_selected_for_device (Option<PlayerId])

enum AssignmentState { AssignedToOtherDevice, Unassigned, UserSelected, AssignedToThisDevice }

fn params_for(player, did, last_selected_for_device) -> (AssignmentState, is_playing, is_last_selected):
  let assign_state =
    if player.assigned_device == Some(did) {
      AssignedToThisDevice
    } else if player.is_assigned_device_attached { # assigned to some other connected device
      AssignedToOtherDevice
    } else if Some(player.id) == preferred_player {
      UserSelected
    } else {
      Unassigned
    }
  let is_playing = (player.state.status == Playing)
  let is_last_selected = (last_selected_for_device == Some(player.id))
  return (assign_state, is_playing, is_last_selected)

# comparator consistent with is_better_selection in orchestrator.rs
fn better_than(a, b_opt): bool {
  if b_opt is None: return true
  let (a_asg, a_play, a_last) = a
  let (b_asg, b_play, b_last) = b_opt.unwrap()

  # identical situation: prefer last selected
  if a_asg == b_asg && a_play == b_play:
    return a_last

  # same assignment category: prefer playing
  if a_asg == b_asg:
    return a_play

  # same playing status: use assignment ordering (AssignedToThis > UserSelected > Unassigned > AssignedToOther)
  if a_play == b_play:
    return a_asg > b_asg

  # playing status differs
  if a_play:
    if a_asg == Unassigned && b_asg == UserSelected: return false
    if a_asg == AssignedToOtherDevice: return false
    return true
  else:
    if a_asg == UserSelected && b_asg == Unassigned: return true
    if b_asg == AssignedToOtherDevice: return true
    return false
}

fn find_player_for_device(did): Option<PlayerId> {
  let last_sel = last_selected_for_device(did)
  let best = None
  let best_params = None
  for p in all_players:
    let pp = params_for(p, did, last_sel)
    if better_than(pp, best_params):
      best = Some(p.id)
      best_params = Some(pp)
  return best
}
```

Implications and clarifications:
- Preferred (UserSelected) affects only players that are not assigned to any connected device and are not assigned to the current device.
  - It does not override a Playing AssignedToThisDevice.
  - When both candidates are Playing, AssignedToThisDevice still beats UserSelected.
  - When both candidates are Non-Playing, UserSelected is promoted and can override even AssignedToThisDevice.
  - It is protected against being preempted by a Playing Unassigned candidate.
- Unassigned devices do not "mirror" a global general selection; they make their own decision using the above comparator. In typical cases, multiple unassigned devices will converge on the same player due to identical inputs.
- Players assigned to other connected devices are effectively excluded unless no better candidates exist; even then, Playing does not help them win.
- If no candidate is selected for a device, the orchestrator applies the default PlayerState (clearing state) to that device when an update is required.

## 1. Terms and Model

Note: Sections 1–10 below are retained as a historical proposal and are not authoritative. They may mention a general-group selection memory, a global fallback to "any playing," and a recompute/apply driver that the current orchestrator does not implement. For the actual, authoritative behavior, see Section 0 above. Where any text below contradicts Section 0 or the code in core/src/orchestrator.rs, Section 0 prevails.

- Player: A media source exposing a PlayerState (status, timeline, metadata).
- Device: An FSCT-compatible output device controlled by the host.
- Assignment: A relation mapping player(s) to device(s). We allow many-to-many assignments.
- Device group: For each connected device D, its group consists of all players assigned to D.
- General group: Players not assigned to any connected device are members of the general group. If a player is assigned to a device that is not connected, that player is treated as unassigned for grouping purposes and thus belongs to the general group.
- Preferred player: A single optional PlayerId that influences selection in the general group.
- Active player (per group): The single selected player whose state is propagated to devices in that group.

## 2. Selection Policy (Requirements)

For each group (per-device or general), the orchestrator selects an active player according to these rules:

1) Preferred for general group
- If a preferred player is set and it belongs to the general group, it becomes the active player for the general group regardless of status.

2) Playing wins if no preferred (or preferred is not a general member)
- If no preferred applies, pick the player(s) within the group whose status is Playing.
- If exactly one player is Playing, select that player.
- If more than one is Playing or none is Playing, select the last player that was treated as active most recently for the group (stability rule).

3) Disconnected assignment handling
- A player assigned to a device that is not connected is considered a member of the general group.

4) Global fallback when general group is empty
- If there are no players in the general group, but there are players in any device groups, then the general selection should be:
  - If any players in the entire system are Playing, select one of those Playing players using the stability rule (keep last if possible). If multiple are Playing, do not thrash; keep the last general selection if it points to a Playing player; otherwise pick a stable candidate.
  - If none are Playing, general active player is None (do not force a selection).

5) Stability rule
- When ambiguous (multiple Playing), keep the previously selected active player for that group if it is still a member; when idle (none Playing), prefer the most recently changed player if available, otherwise keep the last selection; avoid reapplying identical state.

6) Per-device groups vs. general group
- Per-device groups ignore the preferred player. Preferred only affects the general group.
- Unassigned (general) devices mirror the general group’s active player.

## 3. Event Model and Reactions

The orchestrator consumes events from two domains and maintains internal state to compute selections.

### 3.1 Player events
- Registered { player_id, self_id }
  - Add player to the known set. No immediate apply; wait for state.
- Unregistered { player_id }
  - Remove player from all group memberships and selection memories.
  - If the player was preferred, clear preferred.
  - Recompute selections for impacted groups (general and any device group it belonged to).
- Assigned { player_id, device_id }
  - Add membership: player → device; device → player.
  - If the device is connected and player state is known, the device group can immediately apply that player’s state (optional optimization), then recompute for consistency.
- Unassigned { player_id, device_id }
  - Remove membership for that edge only.
  - Recompute selection for the affected device group and for the general group (because the player may move back to general).
- StateUpdated { player_id, state }
  - Cache last known state per player.
  - For each connected device group containing this player, consider applying immediately to that device (if selection points to this player), then recompute to update any dependent groups.
- PreferredChanged { preferred: Option<PlayerId> }
  - Update the preferred player pointer.
  - Recompute selection for the general group and any unassigned devices.

### 3.2 Device events
- Added(device_id)
  - Mark device as connected.
  - Compute and apply selection for this device’s group:
    - If group non-empty: select by the device group rules (Playing/last-active).
    - If group empty: select from the general group rules (preferred, Playing, last-active, global fallback).
- Removed(device_id)
  - Mark device as disconnected.
  - Recompute general selection (players previously considered outside general may now be general if all their assigned devices are disconnected) and any affected device groups.

## 4. Internal State and Data Structures

- last_state_per_player: Map<PlayerId, PlayerState>
- player_to_devices: Map<PlayerId, Set<DeviceId>>
- device_to_players: Map<DeviceId, Set<PlayerId>>
- connected_devices: Set<DeviceId>
- selected_general: Option<PlayerId> (selection memory for the general group)
- selected_per_device: Map<DeviceId, Option<PlayerId>> (selection memory per device)
- preferred_player: Option<PlayerId>

Rationale: Separation of membership and selection memories helps enforce stability and avoid unnecessary device I/O.

## 5. Selection Algorithms (Pseudocode)

### Helper predicates
```
is_playing(pid): last_state_per_player[pid].status == Playing
player_in_any_connected_device(pid):
  any(d in player_to_devices[pid] where d in connected_devices)

candidates_for_device_group(did): sorted(list(device_to_players[did]))

candidates_for_general_group():
  [pid for pid in last_state_per_player.keys()
   if not player_in_any_connected_device(pid)]
```

### Pick function
```
pick_active(candidates, last_selected, preferred_opt, changed_opt):
  if candidates.is_empty(): return None
  if preferred_opt.is_some() and preferred_opt in candidates:
     return preferred_opt
  playing = [pid for pid in candidates if is_playing(pid)]
  if len(playing) == 1: return playing[0]
  if len(playing) >= 2:
     if last_selected in playing: return last_selected
     if last_selected in candidates: return last_selected
     return playing[0]
  # none playing
  if changed_opt in candidates: return changed_opt
  if last_selected in candidates: return last_selected
  return candidates[0]
```

### Recompute and apply
```
recompute_and_apply(focus_device_opt, changed_player_opt):
  # 1) General group
  gen_candidates = candidates_for_general_group()
  if gen_candidates.is_empty():
     all_players = list(last_state_per_player.keys())
     playing_all = [p for p in all_players if is_playing(p)]
     if playing_all.is_empty():
        general_selected = None
     else:
        general_selected = pick_active(playing_all, selected_general, None, changed_player_opt)
  else:
     general_selected = pick_active(gen_candidates, selected_general, preferred_player, changed_player_opt)
  general_changed = (general_selected != selected_general)
  selected_general = general_selected

  # 2) Per-device
  devices = [focus_device_opt] if focus_device_opt else list(connected_devices)
  to_apply = []
  for did in devices:
    group_cand = candidates_for_device_group(did)
    last_sel = selected_per_device.get(did)
    if group_cand.is_empty():
       sel = selected_general
    else:
       sel = pick_active(group_cand, last_sel, None, changed_player_opt)
    changed = sel != last_sel
    selected_per_device[did] = sel
    if sel is not None:
       if changed or (changed_player_opt == sel):
          if sel in last_state_per_player:
             to_apply.append((did, sel))

  # 3) Apply
  for (did, pid) in to_apply:
     apply_to_device(did, last_state_per_player[pid])
```

Notes:
- The general selection is not automatically applied anywhere; it is used when a device group is empty.
- The changed_player optimization ensures timely updates without redundant applications.

## 6. Edge Cases and Sequences

- Multiple Playing in a group: keep last selected if possible; otherwise choose a deterministic Playing candidate.
- No Playing in a group: keep last selection if still in group; otherwise choose deterministic stable candidate (e.g., most recently changed).
- Player assigned to a disconnected device is treated as general for grouping; when the device connects, the player migrates to that device group.
- General group empty but some are Playing in device groups: general selection can reference any Playing across all players; this affects which state unassigned devices will mirror.
- Rapid sequences: register → assign → disconnect/ connect → state changes should not cause oscillations; selection memory provides stability.

## 7. Orchestrator Architecture Proposal

### Responsibilities
- Subscribe to PlayerManager and DeviceManager event streams.
- Maintain state listed in section 4.
- Derive group candidates and compute active selection per group using the algorithm in section 5.
- Apply PlayerState to devices via a PlayerStateApplier abstraction (decouples device I/O from policy).
- Ensure idempotence and avoid duplicate identical applies.

### Components
- Orchestrator (policy + state):
  - Event loop processing PlayerEvent and DeviceEvent.
  - State cache and selection memory.
  - Recompute-and-apply driver.
- PlayerStateApplier:
  - Interface: apply_to_device(device_id, state) → Future<Result<()>>.
  - Implementations: direct device control, or queued worker for backpressure.
- DeviceManager (existing):
  - Provides subscribe(), Added/Removed events, and setters for status/progress/text.
- PlayerManager (existing):
  - Emits Registered/Unregistered/Assigned/Unassigned/StateUpdated/PreferredChanged.

### Concurrency and Ordering
- Use a single-threaded async event loop for deterministic ordering.
- Use biased select to prioritize shutdown; otherwise process events in arrival order.
- Debounce recomputations by coalescing multiple events into a single recompute when possible (optional optimization).

### Persistence and Recovery (optional)
- Persist preferred player and last selections per device to resume behavior after restart.
- Validate presence on startup and clear invalid references.

### Observability
- Structured logs for every selection change per group (device/general) including reason: preferred, playing-singleton, keep-last, fallback.
- Metrics: number of applies, selection change count, duplicates avoided, latency from event to apply.

## 8. Testing Strategy

- Unit tests per rule:
  - Preferred in general group overrides others.
  - Playing singleton selection.
  - Multiple playing stability.
  - No playing stability.
  - Assignment to disconnected device considered general.
  - General empty but global playing fallback.
  - Unassign/assign races do not create duplicate applies.
- Scenario tests:
  - Multiple devices, multiple players: connect/disconnect sequences, preferred toggles, state bursts.
- Property tests (optional): random event sequences maintain invariants (at most one active per group, no applies without known state, etc.).

## 9. Implementation Notes

- Avoid applying when no state is known for selected player.
- Consider deduplication in the applier layer to prevent repeated identical writes.
- Separate selection computation from effect application; record selection changes even if apply fails, but log errors.

## 10. Example Walkthroughs

1) Preferred player in general group:
- Players P1, P2 unassigned; preferred=P2.
- Any unassigned device mirrors P2; device with group members uses its own group rules.

2) Multiple playing in general group:
- P1 and P2 both Playing; last selected was P1 → keep P1; changing P2’s metadata should not flip selection.

3) Unassign causing general to take over:
- D is connected with group {P1}. P2 is unassigned.
- P2 updates but D stays with P1 (assigned).
- Unassign P1 from D → D becomes unassigned and mirrors general; if P2 is Playing or last active, D receives P2.

This document defines the authoritative policy that Orchestrator must implement.



## 11. Note on Current Implementation Behavior (Orchestrator)

The current orchestrator implementation enforces an additional constraint not reflected in earlier drafts of this document:

- Unassigned (general) devices will NOT select a player that is assigned to another connected device, even if that player is Playing. Players assigned to disconnected devices are treated as general members.
- Consequently, when the general group is empty (i.e., no unassigned players), the general active player is None, and unassigned devices will not "borrow" a Playing state from a different device group.

This clarifies and supersedes the previous "global fallback to any playing" idea from the draft. The rationale is to preserve device-group isolation and prevent leaking states across device assignments.

## Appendix A: Reference unit tests in core/src/orchestrator.rs

These unit tests exercise and document the comparator and selection behavior. They serve as executable documentation of the rules above (names as in the tests module):
- is_better_selection_order_independence_three_cases
- is_better_selection_order_independence_six_players_and_sort_stability
- is_better_selection_tie_broken_by_last_selected
- is_better_selection_penalizes_assigned_to_other_device
- is_better_selection_both_playing_assignment_order
- is_better_selection_playing_unassigned_beats_idle_assigned_here
- is_better_selection_playing_user_selected_beats_playing_unassigned
- is_better_selection_last_selected_breaks_tie_when_both_playing_same_assignment
- is_better_selection_four_players_permutation_and_sort
- is_better_selection_all_assigned_to_other_device_picks_playing

These, along with integration-style orchestrator tests (e.g., preferred_player_drives_general_group, general_group_picks_playing_if_no_preferred, general_does_not_pick_playing_assigned_to_other_device), are continuously run in CI and reflect the current behavior.
