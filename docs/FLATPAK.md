# Flatpak on Fedora and GNOME

Tango can be packaged for Linux using the manifest in `packaging/flatpak`.
It builds the current checkout, including local edits, against GNOME 49 and
the Rust SDK extension. Rust crates are pinned and checksummed from `Cargo.lock`;
compilation uses `cargo build --release --frozen` without network access.
Flathub supplies the runtime and build tools. Tango itself is not published there.

## Build and install on Fedora

From a checkout of this repository:

```sh
sudo dnf install flatpak flatpak-builder python3
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user flathub org.gnome.Platform//49 org.gnome.Sdk//49 org.freedesktop.Sdk.Extension.rust-stable//25.08
bash packaging/flatpak/build.sh
flatpak install --user "dist/Tango-$(flatpak --default-arch).flatpak"
flatpak run io.github.felsenuboot.Tango
```

The initial SDK download and release build can take a while and several GB of
disk space. Later builds reuse downloaded sources and unchanged build stages.
Re-run the build and install commands after editing the app; if Flatpak reports
the same app is already installed, use `flatpak install --user --reinstall` with
the bundle path.

To share the app, send `dist/Tango-x86_64.flatpak` to a friend with an Intel/AMD
laptop. They only need Flatpak and the Flathub remote, then the install/run
commands above with the downloaded file path. They do not need Cargo or the SDK.
Installation still needs internet to obtain the GNOME runtime; dictionary data
is downloaded separately on first use. An aarch64 bundle requires an ARM Linux
machine and will not run on an x86_64 laptop.

## Developing without Flatpak

Cargo downloads Rust bindings, but it does not install GTK's C headers or
pkg-config metadata. On Fedora install these once:

```sh
sudo dnf install gcc pkgconf-pkg-config gtk4-devel libadwaita-devel sqlite-devel xz-devel libsecret-devel
rustc --version  # Rust 1.88 or newer is required; dependencies may require newer Rust.
pkg-config --modversion gtk4 libadwaita-1 sqlite3 liblzma libsecret-1
cargo run --locked
```

If Rust is not installed, Fedora provides `rust` and `cargo` packages. If Rust
was installed through rustup, keep using that installation and update it with
`rustup update stable` when required. GTK must be at least 4.12 and libadwaita
at least 1.5. A `gdk4-sys` custom-build failure often means `gtk4-devel` is
missing or too old. Read the final `pkg-config` diagnostic below that error;
the custom-build failure alone does not establish the cause. Do not delete
`Cargo.lock` to solve a missing system dependency.

## Building from a Mac

Flatpak needs Linux kernel facilities and cannot build or run natively on macOS.
Use a Linux VM or the included **Flatpak** GitHub Actions workflow. A VM on an
Apple Silicon Mac normally produces an aarch64 bundle; use an x86_64 Linux
runner or VM for an Intel/AMD Fedora laptop. The workflow builds on Ubuntu
x86_64 and uploads the bundle as an Actions artifact. It can be run manually
once the workflow is on the default branch, and runs on relevant pull requests.

Initial validation on this Mac covers source generation and packaging syntax.
There is no local Flatpak installation or running Docker daemon; a full Linux
build and graphical Fedora smoke test remain required.

## Sandbox behavior and checks

- Wayland, fallback X11 and GPU access support the GTK interface.
- Network access is needed for dictionary downloads and WaniKani.
- Secret Service access stores account tokens in GNOME Keyring.
- File imports/exports use GTK's file dialog portal, without broad home access.
- Settings, databases and cache live below `~/.var/app/io.github.felsenuboot.Tango/`.
  A native installation's data is not automatically migrated.
- Pronunciation is currently unavailable in this package: Tango calls the host
  `spd-say` executable, which is not included or exposed inside the sandbox.

On Fedora, verify launching from the app grid, downloading/searching JMdict,
restarting with saved preferences and lists, importing/exporting a list through
the file chooser, and saving/removing a WaniKani token if using an account.
The script's `--version` check only verifies executable/library loading.

After changing dependencies, regenerate and commit the source list:

```sh
python3 packaging/flatpak/generate-sources.py
python3 packaging/flatpak/generate-sources.py --check
```

The generator only supports crates.io dependencies and rejects other registries
or git sources. For more complex dependency sources, use the upstream
[Flatpak Cargo generator](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo).
See also [Flatpak build documentation](https://docs.flatpak.org/en/latest/building.html)
and [sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html).
