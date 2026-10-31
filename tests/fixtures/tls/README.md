# Local TLS test fixtures

These certificates and private keys are deliberately shared test data generated
by `tests/generate_tls_fixtures.py`. They identify only local test endpoints and
are used by the TLS, mutual TLS and certificate verification tests.

They are not production credentials. Regenerate the set with:

```sh
python tests/generate_tls_fixtures.py
```
