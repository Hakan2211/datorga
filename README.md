# thera

Local-first documentation assistant for psychotherapists (Tauri 2 desktop app).
Plan: [docs/PLAN.md](docs/PLAN.md) (master) + [docs/PLAN-AMENDMENTS.md](docs/PLAN-AMENDMENTS.md) (PTV 3, corrections, build slices).

## Layout

```
crates/core   portable product core: store (SQLCipher), crypto, audit chain, templates, PII checklist, PDF/DOCX
crates/app    Tauri shell: thin commands over core, OS keychain
templates/    report templates as data (PTV 3 built in)
ui/           TanStack Start (SPA mode) + shadcn/ui + Tailwind + TipTap, German-first i18n
```

Rule: `crates/core` never depends on tauri.

## Prerequisites (Windows)

- Rust (stable), Node 22+, Visual Studio Build Tools 2022 (C++), WebView2 (preinstalled on Win 11)
- **Strawberry Perl** for the encrypted (SQLCipher/OpenSSL) build: `winget install StrawberryPerl.StrawberryPerl`
- Later (local Whisper STT): LLVM/clang: `winget install LLVM.LLVM`

## Develop

```sh
cd ui && npm install

npm run app          # encrypted build (needs Strawberry Perl on PATH before Git's perl)
npm run app:plain    # dev-only fallback: database NOT encrypted (the sidebar shows a warning)
npm run app:build    # installer (MSI/NSIS)
```

Tests:

```sh
cargo test -p thera-core                                              # encrypted
cargo test -p thera-core --no-default-features --features plain-sqlite # dev fallback
cd ui && npm run typecheck && npm test
```

`cargo test -p thera-core` also regenerates the TypeScript types in `ui/src/bindings` (ts-rs).

## Dev data & reset

- Data: `%APPDATA%\de.datorga.thera\` (`thera.db`, `master.wrapped`)
- Master key: Windows Credential Manager, entry `de.datorga.thera` / `master-key`

Switching between the plain and the encrypted build needs a fresh database: delete the data folder **and** the
credential entry (`cmdkey /delete:master-key.de.datorga.thera` or via Credential Manager). Use fake patients only
until the Phase 3 trust items are done.
