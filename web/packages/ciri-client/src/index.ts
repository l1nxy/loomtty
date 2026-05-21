// Public surface for `@ciri/client`. Stable shape downstream packages
// (renderer, app) should import from.

export {
  CiriClient,
  type CiriClientOptions,
  type CiriClientState,
  type CiriEvent,
} from "./client.js";

export {
  WebSocketTransport,
  type TransportCallbacks,
  type TransportState,
  type WebSocketLike,
  type WebSocketTransportOptions,
} from "./transport.js";

export {
  HandshakeError,
  encodeClientHello,
  decodeClientHello,
  decodeServerHello,
  type ClientHello,
  type ServerHelloInfo,
  type VersionCompat,
} from "./hello.js";

export {
  CodecError,
  encodeClientMessage,
  decodeServerMessage,
} from "./codec.js";

export type {
  BounceDirection,
  ClientMessage,
  ColumnState,
  ImageDisplayMode,
  LayoutState,
  PaneDetailInfo,
  SessionDetailInfo,
  SessionInfo,
  ServerMessage,
  TemplateInfo,
  TileState,
  WorkspaceState,
} from "./__generated__/types.js";

export {
  CIRI_PKG_VERSION,
  SERVER_HELLO_LEN,
  WIRE_PROTOCOL_VERSION,
} from "./__generated__/fixtures.js";
