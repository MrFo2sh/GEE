# Make a new GAE environment from envs/_template.
#   powershell -NoProfile -File tools/new-env.ps1 -Slug homelab-ops -Title "Homelab Ops"
# Then edit envs/<slug>/src/domain.rs and src/source.rs, and run:
#   cargo run -p <slug> -- --demo
param(
    [Parameter(Mandatory = $true)][string]$Slug,
    [Parameter(Mandatory = $true)][string]$Title
)
$ErrorActionPreference = 'Stop'
if ($Slug -notmatch '^[a-z][a-z0-9-]{1,40}$') { throw "slug must be lowercase letters, digits and dashes" }
$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'envs/_template'
$dst = Join-Path $root "envs/$Slug"
if (Test-Path $dst) { throw "envs/$Slug already exists" }

New-Item -ItemType Directory -Force (Join-Path $dst 'src') | Out-Null
Copy-Item (Join-Path $src 'Cargo.toml') $dst
Copy-Item (Join-Path $src 'src/*.rs') (Join-Path $dst 'src')

# Each environment gets its own port so several can run side by side.
$count = (Get-ChildItem (Join-Path $root 'envs') -Directory).Count
$port = 4790 + $count
$utf8 = New-Object System.Text.UTF8Encoding($false)
foreach ($f in Get-ChildItem $dst -Recurse -File) {
    $text = [IO.File]::ReadAllText($f.FullName)
    $text = $text.Replace('gae-env-template', $Slug).Replace('Template Desk', $Title).Replace('s.port = 4790;', "s.port = $port;")
    [IO.File]::WriteAllText($f.FullName, $text, $utf8)
}

$cargo = Join-Path $root 'Cargo.toml'
$ws = [IO.File]::ReadAllText($cargo)
if ($ws -notmatch [regex]::Escape("`"envs/$Slug`"")) {
    $ws = $ws -replace '(members = \[[^\]]*)\]', "`$1, `"envs/$Slug`"]"
    [IO.File]::WriteAllText($cargo, $ws, $utf8)
}
Write-Output "created envs/$Slug (port $port). Next: edit src/domain.rs and src/source.rs, then: cargo run -p $Slug -- --demo"
