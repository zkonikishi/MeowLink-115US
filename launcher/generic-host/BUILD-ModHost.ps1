param(
    [string]$Cargo = 'cargo',
    [string]$Node = 'node'
)
$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$env:CARGO_BUILD_JOBS = '1'
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_PROFILE_RELEASE_DEBUG = '0'
$env:CARGO_PROFILE_RELEASE_OPT_LEVEL = '0'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
if (!(Test-Path -LiteralPath 'node_modules/typescript/bin/tsc')) { throw 'Run npm ci first (Node 22+).' }
& $Node node_modules/typescript/bin/tsc --noEmit
if ($LASTEXITCODE -ne 0) { throw 'TypeScript check failed.' }
& $Node node_modules/vite/bin/vite.js build
if ($LASTEXITCODE -ne 0) { throw 'Frontend build failed.' }
& $Node --test tests/launcher-mods-ui.test.cjs
if ($LASTEXITCODE -ne 0) { throw 'Mod UI checks failed.' }
& $Cargo test --manifest-path src-tauri/launcher-mod-runtime/Cargo.toml -j 1 --lib -- --test-threads=1
if ($LASTEXITCODE -ne 0) { throw 'Runtime checks failed.' }
& $Cargo build --manifest-path src-tauri/Cargo.toml --release --features prod -j 1
if ($LASTEXITCODE -ne 0) { throw 'Host build failed.' }
