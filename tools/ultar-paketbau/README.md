# ultar-paketbau – packages from upstream, owned by us

Builds an Ultar package (`/pkg/<id>`, later a signed `.uap`) from **unmodified upstream
releases**. Two goals decide every detail:

1. **Independence.** Nothing in the package comes from a distribution. Every program in it
   starts with the package's own loader and finds every library inside the package; the tool
   proves that after each build (*closure*) and runs the package's checks in a system that
   holds nothing but the package.
2. **Easy updates from the original.** A part of a package is an upstream release pinned by
   version and SHA-256. Updating means: `outdated` shows what is newer, two lines change
   (version, hash), `build` and the checks say whether it still works. Our own changes are
   patch files next to the recipe – never a copy of the source, never a fork. Fewer patches
   = cheaper updates; a patch that upstream could take is sent upstream.

## Commands

```text
ultar-paketbau check    <recipe.toml>            is the recipe sound?
ultar-paketbau outdated <recipe.toml>            which parts have a newer upstream release?
ultar-paketbau fetch    <recipe.toml> <workdir>  download the archives, compare (or print) hashes
ultar-paketbau build    <recipe.toml> <workdir>  build every part, then pack
ultar-paketbau pack     <recipe.toml> <workdir>  the shipped copy: trimmed, closure proven, checks run
ultar-paketbau closure  <recipe.toml>            closure of the built tree
```

`build` needs to run as root on the build machine: the package is built at its final place
(`/pkg/<id>`), and the checks run in an empty root (mount namespace + chroot).

## A recipe

`pakete/<id>/paket.toml`: the package (`id`, `prefix`, what the build machine must provide,
environment) and its parts in build order. A part:

| key | meaning |
|---|---|
| `kind` | `tool` – needed to build, installed to the tools directory, **not shipped**; `runtime` – shipped |
| `version`, `source`, `sha256` | the pinned upstream release (`{version}`, `{version_}` in the address) |
| `mirror` | a second address of the same archive |
| `upstream`, `tag` | where upstream tags its releases and what a release tag looks like – for `outdated` |
| `license`, `why` | go into the package's list of contents (`share/ultar/contents.toml`) |
| `patches` | our changes, `*.patch` next to the recipe, applied in order |
| `build` | shell commands; variables `{prefix}` `{tools}` `{build}` `{recipe}` `{jobs}` `{arch}` `{version}` |

`runtime_env` makes the shipped parts use the package's own C library and loader.
`remove` lists what is only needed to build (headers, static libraries): it stays in the built
tree – the next part and the next update build against it – and is left out of the shipped copy.
`[[check]]` is a program built against the package and run where only the package exists.

## What the build machine provides

A C/C++ compiler, make, python3, git, curl, tar, patch, xz and the headers of the Linux kernel
interface. Everything else – including pkg-config, bison, flex, cmake, meson, ninja and the
LLVM tool chain Mesa needs – is built from source by the recipe.

Still taken from the build machine and to be replaced by parts of our own later: the compiler
itself (and with it the C++ standard library, linked statically into the shipped C++ code),
the kernel interface headers.

## Updating a part

```text
ultar-paketbau outdated pakete/grafik-laufzeit/paket.toml
# set the new version, clear sha256
ultar-paketbau fetch    pakete/grafik-laufzeit/paket.toml <workdir>   # prints the new hash
# compare the hash with what upstream publishes, pin it
ultar-paketbau build    pakete/grafik-laufzeit/paket.toml <workdir>
```

A hash printed by `fetch` only says what was downloaded. Where upstream publishes checksums or
signatures, compare before pinning.

## Packages

| package | contents | state |
|---|---|---|
| `grafik-laufzeit` | C library, zlib, libdrm, Vulkan loader, Mesa's Intel Vulkan driver | see `docs` of the recipe and the project notes |
