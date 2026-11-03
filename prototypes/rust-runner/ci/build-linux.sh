#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test "$(rustc --version | cut -d ' ' -f 2)" = '1.99.1'
cargo build --release --locked
cargo test --release --locked
mkdir -p dist
cp target/release/reqon-prototype dist/reqon
cp /etc/ssl/certs/ca-certificates.crt dist/ca-certificates.crt
test "$(dist/reqon --version)" = 'reqon-runner-prototype 0.14.1'
(cd dist && sha256sum reqon ca-certificates.crt > SHA256SUMS)
python3 -c 'import hashlib,json,pathlib; p=pathlib.Path("dist/reqon"); pathlib.Path("dist/reqon-linux-build.json").write_text(json.dumps({"version":"0.14.1","sha256":hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+"\n")'
printf '%s\n' 'Linux binary ready. Run Python acceptance before packaging.'
