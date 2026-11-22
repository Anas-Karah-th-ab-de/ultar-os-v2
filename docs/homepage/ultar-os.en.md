# Ultar OS – an independent operating system, Arabic first

**Ultar OS** is built from the ground up by **KM Code Craft**: its own kernel written in Rust, an Arabic
right-to-left interface, and one code base for phone, computer and server.

## What makes it different

- **Its own kernel.** The Ultar kernel is written in Rust and runs without Linux. In the emulator it is ready in about 145 ms.
- **Arabic first.** Every text is written in Arabic first: the six Arabic plural forms, the Hijri calendar, and
  search that ignores diacritics and spelling variants. Digits are always 0–9.
- **One system for every device.** The system adapts to what the device can do: touch, battery, modem, second
  screen. Desktop mode is available on phones and tablets too.
- **Drivers on the installation medium.** Signed driver packages are selected automatically; installation works without internet.
- **Privacy and security.** Every part runs with the fewest rights possible; no unsigned driver or app is installed.
- **Seven clearly separated areas:** kernel, drivers, system core, system services, platform, system apps, third-party apps.

## Where we are (22 November 2026)

We are rebuilding the system on a new, ordered foundation. The foundation is finished and tested: language
system, device profile, hardware interface, system bus and driver packages. 842 automated tests pass with no
failures, and the system bus runs on the Ultar kernel. The interface and the services still come from the
previous version and are being rebuilt in phases.

Current test devices: a Lenovo ThinkPad P1 laptop, a Beelink GTi15 Ultra mini PC and a Samsung Galaxy A30 phone.

## What comes next

Complete drivers for the two test computers (graphics, Wi-Fi, sound), a new interface in which every app runs
in its own isolated process, and an installer on the level of the established desktop systems.

KM Code Craft · Mohamad Anas Kara Mohamad · info@code-craft.tech
