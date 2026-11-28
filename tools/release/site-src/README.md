# Website ultar.code-craft.tech

Structure (like a product site): Products (Desktop, Server, Phone, Kernel, Mesh, Helper, Store, Security,
Hardware, Developers) · Get Ultar (`/download/…`, `/releases/`) · Guides (`/install/…`, `/update/`) · Help
(`/support/`, `/account/`) · News · About. Arabic at `/`, English at `/en/`, German at `/de/`.

* Texts: `pages.py` (Arabic first, then English and German; no comparisons with other systems; only what is true today).
* Build: `python3 tools/release/site-src/sitegen.py` → `tools/release/site/` (commit the output).
* Dynamic parts (`app.js`): release lists from `/downloads/releases.json` (files sorted into desktop/server, phone,
  VM by name), newest network update from `/v1/update/latest`. No inline scripts or styles (CSP `'self'`).
* Screenshots: `tools/release/site/img/shots/*.webp` from `platform/ultar-desktop/tests/snapshots` (ar/en, dark).
* Deploy: unchanged – `tools/release/vps/deploy-web.sh tools/release/site` on the VPS (keeps `downloads/` and `store/`).
* Account/portal: no link to the customer portal yet; `/account/` says web management is planned.
