# FSCT IPC Implementation Plan

This document outlines the plan to add local IPC communication to the FSCT Host using MessagePack for serialization and an optional MessagePack-RPC style request/response layer. The initial priority is to support a remote implementation of the FsctDriver trait and provide an API to retrieve the protocol version.

## Objectives
- Provide a transport for local-only IPC between FSCT clients (e.g., players, UI, Node bindings) and the host service.
- Use MessagePack (compact, fast) as the wire format.
- Provide a MessagePack-RPC compatible layer (simple request/response with IDs)
- Expose an IPC-backed implementation of `FsctDriver` (client) and a server that forwards to the existing in-process managers.
- Support a `get_protocol_version()` method from the client.

## High-level Architecture
- Server (in-service, native host):
  - Listens on a local IPC endpoint (Windows Named Pipe / Unix domain socket).
  - Accepts client connections.
  - Reads MessagePack frames, dispatches requests to FsctDriver implementation.
- Client (library used by applications):
  - Connects to the local endpoint.
  - Implements `FsctDriver` by encoding RPC-like requests and awaiting responses.

## Transport Options
- `parity-tokio-ipc` crate:
  - Pros: battle-tested, supports Windows Named Pipes and Unix Domain Sockets, async with Tokio.
  - Cons: low-level – framing and protocol are up to us.
- `interprocess` crate: 
  - Pros: modern cross-platform IPC abstractions (UDS, named pipes). 
- Platform-specific: `tokio` with `tokio::net::UnixStream` for Unix and `tokio::net::windows::named_pipe` for Windows.

We will start with `parity-tokio-ipc` as primary, with the code architected so the transport can be swapped if needed.

## Serialization
- `rmp-serde` for MessagePack serialization/deserialization of request/response structs.
- `rmpv` for dynamic/Value-like payloads if needed (e.g., for future extensibility).
- `rmp-rpc` for MessagePack-RPC compatibility layer.

## RPC Layer
- Option A: MessagePack-RPC compatibility:
  - Follow msgpack-rpc specification ("request", "response", "notification" tuples).
  - Uses rmp-rpc, but it is not actively maintained.
- Option B: Lightweight in-house RPC:
  - Define a simple frame format: `[u32 length][MessagePack bytes]`.
  - Message structure:
    - `type`: "request" | "response" | "event"
    - `id`: u64 (for request/response correlation; absent for events)
    - `method`: string (for requests)
    - `params`: MsgPack array/map
    - `result` or `error`: present in responses
  - Advantages: minimal dependencies, explicit control, fits our needs.
  
We will start with Option A

## Versioning and Negotiation
- `get_protocol_version()` is exposed in `FsctDriver` (already added in core).
- On connection, the client gets a version by sending a `get_protocol_version` request.
- Server responds with its version/capabilities. If incompatible (major mismatch), the client closes the connection with an error.
- ProtocolVersion struct: `{ major: u16, minor: u16 }` with constant `FSCT_PROTOCOL_VERSION` currently set to `1.0`.

## Endpoints (Methods)
Map 1:1 to `FsctDriver`:
- `get_protocol_version()` -> request: `method = "get_protocol_version"`, no params, response: `{ major, minor }`.
- `register_player(self_id: String) -> ManagedPlayerId`
- `unregister_player(player_id: ManagedPlayerId)`
- `assign_player_to_device(player_id, device_id)`
- `unassign_player_from_device(player_id, device_id)`
- `update_player_state(player_id, PlayerState)`
- `update_player_status(player_id, FsctStatus)`
- `update_player_timeline(player_id, Option<TimelineInfo>)`
- `update_player_metadata(player_id, FsctTextMetadata, Option<String>)`
- `set_preferred_player(Option<ManagedPlayerId>)`
- `get_preferred_player() -> Option<ManagedPlayerId>`
- `get_player_assigned_device(player_id) -> Option<ManagedDeviceId>`

## Security
- Local only, not network exposed.
- Endpoint paths and permissions:
  - Windows Named Pipe path: `\\.\pipe\fsct_host` (exact name versioned later, e.g., `fsct_host_v1`).
    - Use a secure Security Descriptor to allow access for the current user and Administrators.
  - Unix Domain Socket path: `${XDG_RUNTIME_DIR}/fsct/fsct.sock` (fallback to `/tmp/fsct.sock` if runtime dir not available).
    - Create parent directory with `0700`, socket file `0600`.

## Discovery
- Fixed, documented endpoint names/paths per platform.
- Optionally expose an environment variable override `FSCT_IPC_ENDPOINT` for advanced setups and testing.

## Error Handling and Robustness
- Timeouts on requests.
- Backpressure: bounded channels in server for work dispatch.
- Graceful shutdown: server drains active requests and closes streams; clients retry with exponential backoff.

## Project Changes (Phased)

Phase 0 (Done in this change):
- Add `ProtocolVersion` and `FSCT_PROTOCOL_VERSION`.
- Extend `FsctDriver` with `get_protocol_version()` and implement in `LocalDriver`.

Phase 1 (API/Feature scaffolding):
- Add dependencies:
  - `parity-tokio-ipc` (transport)
  - `rmp-rpc` (RPC layer)

Phase 2 (Server):
- New module `core/src/ipc/server.rs` with `IpcServer`:
  - Accept loop (Tokio task) on endpoint.
  - Per-connection task: decode frames, route to handlers backed by `FsctDriver` implementation (`LocalDriver`)

Phase 3 (Client):
- New module `core/src/ipc/client.rs` with `IpcDriver` implementing `FsctDriver`:
  - Connection management, request/response ID tracking.

Phase 4 (Integration):
- Native service is divided into 2 parts:
  - `driver` (run as root/admin) - implements `FsctDriver` and exposes IPC endpoint.
  - `user-service` (run as user) - implements `FsctDriver` which connects to the IPC endpoint.
- Node.js bindings can either:
  - Use `IpcDriver` via N-API, or
  - Keep current approach and migrate later.

Phase 5 (Testing):
- Unit tests for each function
- Integration tests that spin up in-process server and client, validate end-to-end calls.
- Platform tests:
  - Windows Named Pipe path access/permissions.
  - Unix domain socket path creation and permissions.

## Compatibility and Versioning
- Major version increments for breaking changes in wire protocol.
- Minor for backward-compatible additions.

## Milestones
1. Scaffolding and versioning (complete).
2. Server skeleton accepting connections and returning `get_protocol_version`.
3. Implement a subset of driver calls (register/unregister player).
5. Full driver parity and stabilization.

## Open Questions
- Single shared connection per process vs per driver instance.
- Backpressure strategies for high event rates.
- Multi-user isolation for Windows services (per-session vs machine-wide pipe).
