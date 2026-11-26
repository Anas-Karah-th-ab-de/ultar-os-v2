# Servo for the Ultar browser

The full engine of the browser (JavaScript, CSS, forms) is Servo, embedded as the crate
`servo` from crates.io. The build machine (192.168.100.12, 16 cores) has a slow link that
breaks long downloads, so `git clone` of Servo fails; crates download one by one with retries.

- `fetch-and-probe.sh` – fetches `servo = "0.8"` with all crates (retried), installs the
  system packages and builds a minimal embedder as proof. Log: `/root/servo-build/fetch.log`.

Next: the engine host (`ultar-web` IPC protocol, `SoftwareRenderingContext`, the persona of
`apps/ultar-web/src/privacy` in every JavaScript interface), see the project document
`BROWSER-V2.md`. On the Ultar kernel it needs Mesa (llvmpipe).
