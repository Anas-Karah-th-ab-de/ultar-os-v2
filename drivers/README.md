# drivers/ – area 2

- `manifests/` – one manifest per driver that exists today (built into the Ultar kernel, stage A).
- `manifests/planned/` – manifests of drivers still to be written (see `docs/DRIVER-TARGETS.md`); not packed into the depot.
- `payload/<id>/` – content of each package.
- `depot/build.sh` – packs and signs all `.utr` packages and writes the signed `index.toml` for the installation medium.

Format, signing and matching: `system/core/ultar-driverpkg` (tool `udrv`). Runtime selection with fallback: `system/core/ultar-devd`.
