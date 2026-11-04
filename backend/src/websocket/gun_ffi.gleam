/// FFI bindings for Erlang's gun WebSocket client library
/// WebSocket connection handle (ConnPid + StreamRef)
pub type Connection

/// Frame types received from WebSocket
pub type Frame {
  TextFrame(String)
  BinaryFrame(BitArray)
  CloseFrame(Int, String)
  PingFrame(BitArray)
  PongFrame(BitArray)
}

/// Connect to a WebSocket server
/// host: hostname (e.g., "localhost")
/// port: port number (e.g., 6680)
/// path: WebSocket path (e.g., "/mopidy/ws")
@external(erlang, "gun_ffi_helpers", "connect")
pub fn connect(
  host: String,
  port: Int,
  path: String,
) -> Result(Connection, String)

/// Send a text message over the WebSocket
@external(erlang, "gun_ffi_helpers", "send_text")
pub fn send_text(conn: Connection, text: String) -> Result(Nil, String)

/// Receive a frame from the WebSocket (blocking with timeout in ms)
@external(erlang, "gun_ffi_helpers", "receive_frame")
pub fn receive_frame(conn: Connection, timeout: Int) -> Result(Frame, String)

/// Close the WebSocket connection
@external(erlang, "gun_ffi_helpers", "close")
pub fn close(conn: Connection) -> Nil
