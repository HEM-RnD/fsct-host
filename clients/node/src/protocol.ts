// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

// Internal module: JSON-RPC 2.0 message types and framing constants.

/** Maximum allowed line length in bytes (matches server-side MAX_LINE_BYTES) */
export const MAX_LINE_BYTES = 1 << 20; // 1 MiB

/** Expected major protocol version; minor differences are backwards-compatible */
export const EXPECTED_PROTOCOL_MAJOR = 1;

// JSON-RPC 2.0 error codes
export const ERR_PARSE_ERROR = -32700;
export const ERR_INVALID_REQUEST = -32600;
export const ERR_METHOD_NOT_FOUND = -32601;
export const ERR_INVALID_PARAMS = -32602;
export const ERR_APPLICATION = -32000;

export interface RpcRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params: unknown;
}

export interface RpcResponse {
  jsonrpc: '2.0';
  id: unknown;
  result?: unknown;
  error?: RpcError;
}

export interface RpcError {
  code: number;
  message: string;
}

export interface RpcNotification {
  jsonrpc: '2.0';
  method: string;
  params: unknown;
}

export function isRpcResponse(obj: unknown): obj is RpcResponse {
  return (
    typeof obj === 'object' &&
    obj !== null &&
    'id' in obj &&
    'jsonrpc' in obj &&
    !('method' in obj)
  );
}

export function isRpcNotification(obj: unknown): obj is RpcNotification {
  return (
    typeof obj === 'object' &&
    obj !== null &&
    'method' in obj &&
    'jsonrpc' in obj &&
    !('id' in obj)
  );
}

/** Thrown when the driver returns a JSON-RPC error response */
export class FsctError extends Error {
  constructor(
    public readonly code: number,
    message: string,
  ) {
    super(message);
    this.name = 'FsctError';
  }
}
