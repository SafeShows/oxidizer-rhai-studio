# Oxidizer Rust mod template

A starting point for a [Kitten Space Agency](https://ksa.ahwoo.com) mod written in Rust, run by
[Oxidizer](https://github.com/SafeShows/Oxidizer) on Windows, Linux and macOS.

The example mod opens a window with the controlled vehicle and the game time, adds an entry to the game's **Mods**
menu, toggles with **F8**, and hooks a game method.

## Start

1. Install Oxidizer: as a StarMap mod (`Oxidizer.StarMap`) or standalone, see
   [Oxidizer's install steps](https://github.com/SafeShows/Oxidizer#install). Install
   [Rust](https://rustup.rs) too.
2. Click **Use this template** on GitHub to make your own repository from this one, and clone it.
3. Build and install the mod into the game's mods folder (quit the game first on Windows):
   ```sh
   ./install.ps1    # Windows
   ./install.sh     # Linux, macOS
   ```
   Both take the mods folder as an argument if the game's folder isn't in the usual place.
4. Start the game, and enable the mod when it asks. **Mods > Oxidizer** shows errors and the log.

## Make it yours

The mod's id is its folder name in `mods/`, and here it is the package name in `Cargo.toml`. To rename it from
`my_mod`, change these together:
- `name` in `Cargo.toml`,
- `name` and the `dll` path in `mod/mod.toml`,
- `id` in `index-listing.example.toml`.

The install scripts and the release workflow read the id from `Cargo.toml`.

`LICENSE` is MIT No Attribution, so you can relicense your mod however you like: replace it with your own.

## Write the mod

`src/lib.rs` implements `oxidizer_api::Mod`: `init`, `frame`, `gui`, `menu` and `unload`, all but `init` optional.
The API (game objects, hooks and their return values, ImGui, keys, settings) is on
[docs.rs](https://docs.rs/oxidizer_api); [Oxidizer's README](https://github.com/SafeShows/Oxidizer#rust) has an
overview.

The template uses the by-name API, `Obj::static_get("KSA.Program, KSA", "ControlledVehicle")`. For typed access to
the whole game, with autocomplete and compile-time checks, generate bindings for your installed game:
```sh
oxidizer --bindgen src/ksa.rs     # with standalone Oxidizer; again after each game update
```
```rust
mod ksa;
use ksa::ksa::{Program, Universe};

let vehicle = Program::controlled_vehicle(); // a typed ksa::ksa::Vehicle
let time = Universe::get_elapsed_seconds();
```
`src/ksa.rs` is in `.gitignore` because it belongs to one game version; CI doesn't have the game to generate it,
so a release build needs it committed (or keep using the by-name API).

Keep the library in `mod/`'s `bin/` folder, as `mod.toml` says: StarMap tries to load `mods/<id>/<id>.dll` as a C#
mod, and a native library there stops the game.

## Release

Push a tag:
```sh
git tag v1.0.0
git push origin v1.0.0
```
The workflow builds the mod for Windows, Linux and macOS and publishes a release with `my_mod-1.0.0.zip`, holding a
`my_mod/` folder with `mod.toml`, all three libraries in `bin/`, and `THIRD-PARTY-NOTICES.txt` with the licenses of the crates compiled into them (`scripts/notices.py`; forum rules ask for that attribution), the layout mod managers install.

## List it in the content index

So players can find and install the mod with managers like Borea, list it in the
[KSA content index](https://github.com/KSAModding/content-index): copy `index-listing.example.toml` to
`listings/<id>.toml` in a fork of that repository, fill it in, and open a pull request. Publish your release
**first**, because the index checks it when you open the pull request. The listing also needs a thread for the mod
on the [KSA forums](https://forums.ahwoo.com).
