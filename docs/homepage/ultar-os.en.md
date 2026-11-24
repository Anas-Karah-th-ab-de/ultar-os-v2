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

## New since 24 November 2026: device mesh and local helper

- **Device mesh.** Your own devices find each other, are paired once with a six-digit code and hand work
  over: what is open on one device continues on the other. Every connection is end-to-end encrypted.
  Nothing arrives unasked – the receiving device always asks. Built so far: the service (also through a
  relay server for devices in different networks), the page "Mesh" in Settings with pairing and rights per
  device, the tile in the quick settings, "Continue on another device" in the app's menu, and the question
  on the receiving device. The mesh has not yet been tried on real devices; the phone surface and remote
  view are still missing.
- **Local helper.** You ask in words, the helper acts through the system bus – with narrow rights of its
  own and never without confirmation for anything dangerous. It adapts to the device: with its own language
  model it thinks itself; a weak device uses the model a paired device lends; without either it understands
  fixed commands and answers questions about the system in Arabic, English and German. It can also ask the
  helper of a paired device to act there, when that device allows it. The service is built and tested; a
  surface of its own and voice control are still missing.

## What comes next

Complete drivers for the two test computers (graphics, Wi-Fi, sound), a new interface in which every app runs
in its own isolated process, and an installer on the level of the established desktop systems.

KM Code Craft · Mohamad Anas Kara Mohamad · info@code-craft.tech
