import gleam/bytes_tree
import gleam/http
import gleam/http/response
import gleam/list
import gleam/option.{None, Some}
import gleeunit/should
import handlers/artwork
import mist

pub fn local_artwork_uses_configured_mopidy_origin_test() {
  let assert Ok(req) =
    artwork.upstream_request(
      "ws://mopidy:6680/mopidy/ws?token=not-an-image-token",
      "5ef5b1ab4e1759485cdac6867e722aca-600x600.png",
    )
  req.scheme |> should.equal(http.Http)
  req.host |> should.equal("mopidy")
  req.port |> should.equal(Some(6680))
  req.path
  |> should.equal("/local/5ef5b1ab4e1759485cdac6867e722aca-600x600.png")
  req.query |> should.equal(None)
  req.headers |> should.equal([])
}

pub fn secure_mopidy_artwork_uses_https_test() {
  let assert Ok(req) =
    artwork.upstream_request("wss://music.example/mopidy/ws", "cover.jpg")
  req.scheme |> should.equal(http.Https)
  req.host |> should.equal("music.example")
  req.path |> should.equal("/local/cover.jpg")
}

pub fn artwork_rejects_traversal_and_non_images_test() {
  [
    "",
    "..",
    "../cover.png",
    "%2e%2e%2fcover.png",
    "cover.png?uri=http://other-host",
    "//other-host/cover.png",
    "cover\\image.png",
    "track.mp3",
    "config.json",
    "cover.svg",
  ]
  |> list.each(fn(filename) {
    artwork.upstream_request("ws://mopidy:6680/mopidy/ws", filename)
    |> should.be_error
  })
  artwork.upstream_request("file:///etc/passwd", "cover.png")
  |> should.be_error
  artwork.upstream_request("ws:///mopidy/ws", "cover.png")
  |> should.be_error
}

pub fn artwork_preserves_binary_bytes_and_safe_headers_test() {
  let image = <<137, 80, 78, 71, 13, 10, 26, 10, 255, 0>>
  let upstream =
    response.new(200)
    |> response.set_header("content-type", "image/png")
    |> response.set_header("set-cookie", "upstream-cookie")
    |> response.set_body(image)
  let proxied = artwork.image_response(upstream)
  proxied.status |> should.equal(200)
  response.get_header(proxied, "content-type") |> should.equal(Ok("image/png"))
  response.get_header(proxied, "x-content-type-options")
  |> should.equal(Ok("nosniff"))
  response.get_header(proxied, "set-cookie") |> should.be_error
  let assert mist.Bytes(body) = proxied.body
  bytes_tree.to_bit_array(body) |> should.equal(image)
}

pub fn artwork_does_not_forward_html_or_redirects_test() {
  let html =
    response.new(200)
    |> response.set_header("content-type", "text/html")
    |> response.set_body(<<>>)
  artwork.image_response(html).status |> should.equal(502)
  artwork.image_response(response.new(302) |> response.set_body(<<>>)).status
  |> should.equal(502)
  artwork.image_response(response.new(404) |> response.set_body(<<>>)).status
  |> should.equal(404)
}
