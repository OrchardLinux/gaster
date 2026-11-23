# Orchard Gaster worker

This builds the executable Orchard runs as a subprocess. It is separate from
the upstream `gaster` CLI; the root Makefile and its targets are unchanged.

From the repository root, run:

```sh
make -C orchard-worker
make -C orchard-worker check
```

The executable is `orchard-worker/target/worker/gaster-worker`. The `check`
target runs the worker's self-test without accessing USB. Builds require Cargo
and a C compiler. Linux also requires `pkg-config` and libusb development files;
the Linux executable needs libusb at runtime. The macOS build uses the system
CoreFoundation and IOKit frameworks.

`worker.c` and `generate.rs` came from Orchard's `crates/device` at revision
`5998317`. The generator verifies the source and payloads against the original
pins from `HoolockLinux/gaster@2b9d747f2367f8c0860def02ec4fd299e531c52a` and
writes generated files under `target/worker/`. Review any change to the upstream
revision and its pins together. Keep the worker's command-line and reply
contract compatible with Orchard's Rust caller when producing a release.

Upstream Gaster is Apache-2.0 (see `../LICENSE`). The Orchard files retain
the GPL-2.0-or-later license declared in their source workspace. Releases of
the combined worker use GPL-3.0 (see `COPYING`).

## Release a worker

The **Orchard Gaster worker** Actions workflow builds and self-tests native
binaries for macOS and Linux on `aarch64` and `x86_64`. Run it manually on the
branch for a build-only check. Once that revision is on the remote, push a new
version tag, for example:

```sh
git tag -a orchard-worker-v0.1.0 -m 'Orchard Gaster worker v0.1.0'
git push origin orchard-worker-v0.1.0
```

The tag run creates a draft GitHub release with four raw executables,
`SHA256SUMS`, and both license notices. Review and publish the draft; the release
notes list the exact sizes and hashes Orchard pins. Use a new tag for any
change to an already published binary. Before deleting Orchard's bundled
worker, verify a cold download and a cached offline run using the published
assets.
