//// Narrow binary proxy for Mopidy-Local album covers. Never proxies music or RPC.

import gleam/bytes_tree
import gleam/http/request.{type Request}
import gleam/http/response.{type Response}
import gleam/httpc
import gleam/list
import gleam/option.{None, Some}
import gleam/result
import gleam/string
import gleam/uri
import handlers/http as h
import mist.{type ResponseData}

/// Build a request to the configured Mopidy origin, not a caller-supplied host.
/// Only a single safe PNG/JPEG filename can be requested beneath /local/.
pub fn upstream_request(
  mopidy_url: String,
  filename: String,
) -> Result(Request(BitArray), Nil) {
  let safe_name =
    filename != ""
    && string.length(filename) <= 255
    && !string.starts_with(filename, ".")
    && !string.contains(filename, "..")
    && list.all(string.to_graphemes(filename), fn(char) {
      string.contains(
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.",
        char,
      )
    })
  let image_name =
    string.ends_with(string.lowercase(filename), ".png")
    || string.ends_with(string.lowercase(filename), ".jpg")
    || string.ends_with(string.lowercase(filename), ".jpeg")
  use _ <- result.try(case safe_name && image_name {
    True -> Ok(Nil)
    False -> Error(Nil)
  })
  use upstream <- result.try(uri.parse(mopidy_url))
  use scheme <- result.try(case upstream.scheme {
    Some("ws") | Some("http") -> Ok("http")
    Some("wss") | Some("https") -> Ok("https")
    _ -> Error(Nil)
  })
  use _ <- result.try(case upstream.host {
    Some(host) if host != "" -> Ok(Nil)
    _ -> Error(Nil)
  })
  let target =
    uri.Uri(
      ..upstream,
      scheme: Some(scheme),
      userinfo: None,
      path: "/local/" <> filename,
      query: None,
      fragment: None,
    )
  target
  |> request.from_uri
  |> result.map(fn(req) { request.set_body(req, <<>>) })
}

/// Preserve binary image bytes; don't expose upstream HTML, headers or redirects.
pub fn image_response(upstream: Response(BitArray)) -> Response(ResponseData) {
  let content_type =
    upstream
    |> response.get_header("content-type")
    |> result.unwrap("")
    |> string.split(";")
    |> list.first
    |> result.unwrap("")
    |> string.trim
    |> string.lowercase
  case upstream.status, content_type {
    200, "image/png" | 200, "image/jpeg" ->
      response.new(200)
      |> response.set_header("content-type", content_type)
      |> response.set_header("cache-control", "public, max-age=3600")
      |> response.set_header("x-content-type-options", "nosniff")
      |> response.set_body(mist.Bytes(bytes_tree.from_bit_array(upstream.body)))
    404, _ -> h.error_response("Album art not found", 404)
    _, _ -> h.error_response("Could not fetch album art from Mopidy", 502)
  }
}

pub fn get(mopidy_url: String, filename: String) -> Response(ResponseData) {
  case upstream_request(mopidy_url, filename) {
    Error(_) -> h.error_response("Invalid album art request", 400)
    Ok(req) -> {
      let config = httpc.configure() |> httpc.timeout(5000)
      case httpc.dispatch_bits(config, req) {
        Ok(upstream) -> image_response(upstream)
        Error(_) -> h.error_response("Could not reach Mopidy album art", 502)
      }
    }
  }
}
