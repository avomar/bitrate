# Bitrate

A Bandwidth Monitor applet for the COSMIC™ desktop.

<img width="549" height="571" alt="image" src="https://github.com/Aviral-Omar/bitrate/blob/main/res/screenshots/view_window_expanded.png?raw=true" />

## Network selection

The applet hides Docker interfaces, virtual Ethernet pairs, and network bridges by default.
Turn off **Ignore Docker & Virtual Networks** to include them in the interface dropdown.
This only filters the applet's list; it does not disable networking on your system.

Without a saved interface choice, the applet follows the lowest-metric active IPv4
default route among visible, connected interfaces. If no such route is available,
it prefers Ethernet, then Wi-Fi. A selection made in the existing dropdown is
remembered while available; if it disconnects or is hidden, the applet falls back
to an available connection.

## Installation

A [justfile](./justfile) is included by default for the [casey/just][just] command runner.

- `just` builds the application with the default `just build-release` recipe
- `just run` builds and runs the application
- `just install` installs the project into the system
- `just vendor` creates a vendored tarball
- `just build-vendored` compiles with vendored dependencies from that tarball
- `just check` runs clippy on the project to check for linter warnings
- `just check-json` can be used by IDEs that support LSP

## Translators

[Fluent][fluent] is used for localization of the software. Fluent's translation files are found in the [i18n directory](./i18n). New translations may copy the [English (en) localization](./i18n/en) of the project, rename `en` to the desired [ISO 639-1 language code][iso-codes], and then translations can be provided for each [message identifier][fluent-guide]. If no translation is necessary, the message may be omitted.

## Packaging

If packaging for a Linux distribution, vendor dependencies locally with the `vendor` rule, and build with the vendored sources using the `build-vendored` rule. When installing files, use the `rootdir` and `prefix` variables to change installation paths.

```sh
just vendor
just build-vendored
just rootdir=debian/bitrate prefix=/usr install
```

It is recommended to build a source tarball with the vendored dependencies, which can typically be done by running `just vendor` on the host system before it enters the build environment.

## Developers

Developers should install [rustup][rustup] and configure their editor to use [rust-analyzer][rust-analyzer].

[fluent]: https://projectfluent.org/
[fluent-guide]: https://projectfluent.org/fluent/guide/hello.html
[iso-codes]: https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes
[just]: https://github.com/casey/just
[rustup]: https://rustup.rs/
[rust-analyzer]: https://rust-analyzer.github.io/
[sccache]: https://github.com/mozilla/sccache

Network discovery tests use isolated filesystem fixtures and do not change host
networking. Run them with `cargo test network::tests` or `./simulate.sh`.
`./simulate.sh live` launches an interactive mock network session for UI testing.
