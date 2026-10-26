# Third-party notices

`kubectl-db3` is distributed under the MIT License (see [LICENSE.md](LICENSE.md)).
The compiled binary also contains third-party software, listed here because
their licenses require the notices to travel with binary distributions.

Nothing in this file grants rights to any third-party trademark. See
[TRADEMARKS.md](TRADEMARKS.md).

## Embedded web console

The `kubectl db3 webui` frontend is compiled and inlined into the binary as a
single HTML file. Minification removes the license banners that ship with the
libraries, so the notices are reproduced here.

The bundled runtime code is:

- `react` 19.x
- `react-dom` 19.x
- `scheduler` 0.2x

### React, React DOM, and Scheduler — MIT License

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

The build toolchain (Vite, TypeScript, `vite-plugin-singlefile`, type packages)
runs only at development time and is not part of the distributed binary or
bundle.

## Statically linked Rust crates

The binary links the Rust crates resolved in `Cargo.lock`. At the time of
writing every dependency is offered under a permissive license, and none is
Apache-2.0-only:

- MIT, or MIT OR Apache-2.0 (most crates, including clap, serde, serde_json,
  chrono, tempfile, ctrlc, and tiny_http)
- Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (`unicode-ident` style
  crates)
- MIT OR Apache-2.0 AND Unicode-3.0 (Unicode data crates)

`Cargo.lock` is the authoritative list of names and exact versions; each
crate's `LICENSE`/`LICENSE-MIT`/`LICENSE-APACHE` file is available in its
registry package on <https://crates.io>. Where a crate is dual-licensed, this
project relies on the MIT option, so no Apache-2.0 `NOTICE` file is reproduced
here; if you reuse this project under the Apache-2.0 option of any dependency,
review that dependency's notices yourself.

Run `cargo metadata --format-version 1 --locked` to regenerate the current
dependency license report.
