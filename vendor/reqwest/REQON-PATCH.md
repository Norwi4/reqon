# Reqon patch to reqwest 0.13.5

Adds `ClientBuilder::omit_headers` for explicitly disabled generated headers.
The HTTP transport removes these after default header insertion and compression
negotiation, while retaining response decompression. For Host it also disables
Hyper's automatic HTTP/1 Host insertion. HTTP/2 authority and body framing remain
managed by the transport. The default behavior is unchanged.

Covered by Reqon's wire-level tests in `src/http_request.rs`.
