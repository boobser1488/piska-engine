<#
.SYNOPSIS
  Piska Engine installer for Windows: installs and configures everything a developer needs.

.DESCRIPTION
  Components (see install/packages.toml, the single source of versions, URLs and checksums):
    core        Rust (rustup, stable, MSVC), Visual Studio Build Tools (C++), Git, Vulkan check
    cpp         CMake + Ninja
    web         wasm32-unknown-unknown target
    android     JDK 17, Android SDK + NDK, cargo-ndk, Rust Android target, ANDROID_HOME ...
    linux-cross zig (checksum pinned and verified), x86_64-unknown-linux-gnu target, PISKA_ZIG
    engine      clone / update the sources, build hub + editor + modeler, shortcuts, uninstaller
  Every step detects first and skips what is already there. -DryRun prints every command,
  download, checksum and environment change and touches nothing.

.EXAMPLE
  .\install.ps1                              # interactive menu
  .\install.ps1 -Components core,web -Yes    # no questions (licences still need -AcceptLicenses)
  .\install.ps1 -Components all -DryRun      # show the plan only
  .\install.ps1 -Resume                      # continue after a failure or a reboot
  .\install.ps1 -Uninstall                   # remove what the installer added

  From the network (read the script first; this is what "irm | iex" does, with parameters):
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/boobser1488/piska-engine/main/install/install.ps1))) -Components core -DryRun
#>
[CmdletBinding()]
param(
    [string[]]$Components,
    [switch]$Yes,
    [switch]$DryRun,
    # Plan as if nothing were installed (implies -DryRun): used by the golden tests.
    [switch]$AssumeClean,
    [string]$InstallDir,
    # Folder for downloaded files; a file that is already there and passes its checksum is reused (offline use).
    [string]$Cache,
    # Where install.ps1 / packages.toml come from when run without a checkout.
    [string]$BaseUrl,
    [string]$AndroidSdk,
    [switch]$AcceptLicenses,
    [switch]$Resume,
    [switch]$Repair,
    [switch]$Uninstall,
    [switch]$NoPrefs,
    [switch]$NoShortcuts,
    [switch]$DesktopShortcut,
    [switch]$FileAssociations,
    # Put `[build] target-dir` into ~/.cargo/config.toml (a delimited block, with a backup).
    [string]$CargoTargetDir,
    [switch]$ListComponents,
    # Machine-readable output for the graphical installer: one JSON object per line on stdout
    # (hello, plan, step, progress, log, prompt, licence, elevation, reboot, uninstall-plan, error, summary).
    # Questions are asked as "prompt" events and answered with one line on stdin ("yes" / "no").
    [switch]$Json,
    # Licences accepted by id (vs-buildtools, android-sdk); -AcceptLicenses accepts every licence.
    [string[]]$AcceptLicense,
    # Answer "Continue?" before a step that needs administrator rights (Windows still shows its own UAC prompt).
    [switch]$ConfirmElevation,
    # Skip these steps on request (only steps listed in $SkippableSteps; the others are refused).
    [string[]]$SkipSteps,
    # Use this engine checkout as the sources instead of cloning into <InstallDir>\engine.
    [string]$SourceDir
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$script:ExitStepFailed = 1; $script:ExitBadArgs = 2; $script:ExitReboot = 3; $script:ExitDeclined = 4
$AllComponents = @('core', 'cpp', 'web', 'android', 'linux-cross', 'engine')
# Steps whose absence does not break the steps after them: -SkipSteps may skip only these.
$script:SkippableSteps = @('core.rust-components', 'core.vulkan', 'engine.shortcuts', 'engine.assoc', 'engine.uninstaller', 'config.preferences', 'config.cargo')
$script:Json = [bool]$Json
$script:CurrentStep = $null
$script:Stats = @{ total = 0; done = 0; skipped = 0; failed = 0 }
$script:FailedStep = $null; $script:FailedError = $null; $script:ElevationRefused = $false
$script:Opt = @{}; $script:Dirs = $null; $script:LogFile = $null; $script:RebootNeeded = $false; $script:RepoDir = $null; $script:InCheckout = $false

# ----------------------------------------------------------------------------------------
# Data file (install/packages.toml): a subset of TOML shared with install.sh and `piska-export doctor`
# ----------------------------------------------------------------------------------------

function ConvertFrom-PiskaToml {
    param([string]$Text)
    $doc = [ordered]@{}
    $section = $null
    foreach ($raw in ($Text -split "`r?`n")) {
        $line = $raw.Trim()
        if ($line -eq '' -or $line.StartsWith('#')) { continue }
        if ($line -match '^\[([^\]]+)\]$') {
            $section = $Matches[1].Trim()
            if (-not $doc.Contains($section)) { $doc[$section] = [ordered]@{} }
            continue
        }
        if ($line -match '^([A-Za-z0-9_\-]+)\s*=\s*(.+)$') {
            if ($null -eq $section) { throw "packages.toml: value outside a section: $line" }
            $key = $Matches[1]; $val = $Matches[2].Trim()
            if ($val.StartsWith('"')) { $doc[$section][$key] = $val.Substring(1, $val.LastIndexOf('"') - 1) }
            elseif ($val.StartsWith('[')) {
                $items = @([regex]::Matches($val, '"([^"]*)"') | ForEach-Object { $_.Groups[1].Value })
                $doc[$section][$key] = [string[]]$items
            }
            elseif ($val -match '^\d+$') { $doc[$section][$key] = [int]$val }
            else { throw "packages.toml: cannot read value: $line" }
        }
    }
    return $doc
}

# ----------------------------------------------------------------------------------------
# Pure helpers (unit-tested by install/tests/run.ps1)
# ----------------------------------------------------------------------------------------

function Get-VersionFromText {
    param([string]$Text)
    if ($Text -match '(\d+(?:\.\d+)+)') { return [version]($Matches[1] -replace '^(\d+(?:\.\d+){0,3}).*$', '$1') }
    return $null
}

function Test-VersionAtLeast {
    param($Found, [string]$Minimum)
    if ($null -eq $Found) { return $false }
    $min = [version]$Minimum
    return ([version]$Found) -ge $min
}

function Resolve-Components {
    param([string[]]$Requested, $Data)
    $result = New-Object System.Collections.ArrayList
    $want = @()
    foreach ($item in $Requested) { foreach ($part in ($item -split ',')) { if ($part.Trim()) { $want += $part.Trim().ToLowerInvariant() } } }
    if ($want -contains 'all') { $want = $AllComponents }
    foreach ($w in $want) { if ($AllComponents -notcontains $w) { throw "unknown component '$w'; known: $($AllComponents -join ', '), all" } }
    function Add-WithNeeds($name) {
        if ($result.Contains($name)) { return }
        foreach ($need in $Data["component.$name"]['needs']) { Add-WithNeeds $need }
        [void]$result.Add($name)
    }
    foreach ($c in $AllComponents) { if ($want -contains $c) { Add-WithNeeds $c } }
    return [string[]]$result.ToArray()
}

function Resolve-MenuChoice {
    param([string]$Answer, [string[]]$Items, [string[]]$Default)
    $a = $Answer.Trim().ToLowerInvariant()
    if ($a -eq '') { return $Default }
    if ($a -eq 'a' -or $a -eq 'all') { return $Items }
    $picked = @()
    foreach ($p in ($a -split '[,\s]+')) {
        if ($p -match '^\d+$' -and [int]$p -ge 1 -and [int]$p -le $Items.Count) { $picked += $Items[[int]$p - 1] }
        elseif ($Items -contains $p) { $picked += $p }
        elseif ($p) { throw "'$p' is not a menu item" }
    }
    return [string[]]($picked | Select-Object -Unique)
}

function Get-FileSha256 {
    param([string]$Path)
    # Get-FileHash exists since PowerShell 4; kept explicit so tests do not depend on the module.
    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha = [System.Security.Cryptography.SHA256]::Create()
        try { return (($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) -join '') } finally { $sha.Dispose() }
    } finally { $stream.Dispose() }
}

function Test-FileChecksum {
    # $true only when the file exists and its SHA-256 equals $Expected (64 hex digits, case-insensitive).
    param([string]$Path, [string]$Expected)
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return $false }
    if ($Expected -notmatch '^[0-9a-fA-F]{64}$') { return $false }
    return (Get-FileSha256 -Path $Path) -eq $Expected.ToLowerInvariant()
}

function Get-Sha256FromText {
    # First 64-hex token of a publisher's checksum file ("<hash>  name" or just "<hash>").
    param([string]$Text)
    if ($Text -match '(?i)\b([0-9a-f]{64})\b') { return $Matches[1].ToLowerInvariant() }
    return $null
}

function Get-ZigIndexSha {
    # index.json of ziglang.org: version -> platform -> shasum.
    param([string]$Json, [string]$Version, [string]$Key)
    $index = $Json | ConvertFrom-Json
    $release = $index.PSObject.Properties[$Version]
    if ($null -eq $release) { return $null }
    $platform = $release.Value.PSObject.Properties[$Key]
    if ($null -eq $platform) { return $null }
    return [string]$platform.Value.shasum
}

function Assert-Download {
    # Throws unless the packages.toml entry carries a checksum source: the installer never runs unverified files.
    param($Entry, [string]$Name)
    $hasPin = $Entry.Contains('sha256') -and $Entry['sha256'] -match '^[0-9a-f]{64}$'
    $hasUrl = $Entry.Contains('sha256_url') -and $Entry['sha256_url'] -match '^https://'
    if (-not ($hasPin -or $hasUrl)) { throw "download '$Name' has neither a pinned sha256 nor a sha256_url in packages.toml; refusing to use it" }
    if ($Entry['url'] -notmatch '^https://') { throw "download '$Name' is not an https:// URL" }
}

function Add-DelimitedBlock {
    # Append a block between BEGIN / END markers. Never changes the existing text; a block with this id is not added twice.
    param([string]$Text, [string]$Id, [string]$Block)
    $begin = "# >>> piska-engine installer: $Id >>>"
    $end = "# <<< piska-engine installer: $Id <<<"
    if ($Text.Contains($begin)) { return $Text }
    $sep = ''
    if ($Text.Length -gt 0 -and -not $Text.EndsWith("`n")) { $sep = "`n" }
    return $Text + $sep + "`n" + $begin + "`n" + $Block.TrimEnd() + "`n" + $end + "`n"
}

function Remove-DelimitedBlock {
    param([string]$Text, [string]$Id)
    $pattern = '(?s)\r?\n?# >>> piska-engine installer: ' + [regex]::Escape($Id) + ' >>>.*?# <<< piska-engine installer: ' + [regex]::Escape($Id) + ' <<<\r?\n?'
    return [regex]::Replace($Text, $pattern, '')
}

# ---- machine-readable output (-Json) ----

function ConvertTo-JsonText {
    # Compact JSON, pure ASCII (every non-ASCII character as \uXXXX), so the console code page cannot damage it.
    param($Value)
    if ($null -eq $Value) { return 'null' }
    if ($Value -is [bool]) { if ($Value) { return 'true' } else { return 'false' } }
    if ($Value -is [int] -or $Value -is [long] -or $Value -is [double] -or $Value -is [decimal] -or $Value -is [int16] -or $Value -is [uint32] -or $Value -is [uint64]) {
        return [string]::Format([Globalization.CultureInfo]::InvariantCulture, '{0}', $Value)
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $parts = @(foreach ($k in $Value.Keys) { (ConvertTo-JsonText ([string]$k)) + ':' + (ConvertTo-JsonText $Value[$k]) })
        return '{' + ($parts -join ',') + '}'
    }
    if ($Value -is [string] -or $Value -is [char]) {
        $sb = New-Object System.Text.StringBuilder
        [void]$sb.Append('"')
        foreach ($c in ([string]$Value).ToCharArray()) {
            $code = [int]$c
            if ($c -eq '"') { [void]$sb.Append('\"') }
            elseif ($c -eq '\') { [void]$sb.Append('\\') }
            elseif ($code -lt 32 -or $code -gt 126) { [void]$sb.Append(('\u{0:x4}' -f $code)) }
            else { [void]$sb.Append($c) }
        }
        [void]$sb.Append('"')
        return $sb.ToString()
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        $parts = @(foreach ($item in $Value) { ConvertTo-JsonText $item })
        return '[' + ($parts -join ',') + ']'
    }
    if ($Value -is [pscustomobject]) {
        $o = [ordered]@{}; foreach ($p in $Value.PSObject.Properties) { $o[$p.Name] = $p.Value }
        return (ConvertTo-JsonText $o)
    }
    return (ConvertTo-JsonText ([string]$Value))
}

function Write-Event {
    # One event = one line on stdout: {"event":"<name>", ...fields}.
    param([string]$Name, $Fields = @{})
    $o = [ordered]@{ event = $Name }
    foreach ($k in $Fields.Keys) { $o[$k] = $Fields[$k] }
    [Console]::Out.WriteLine((ConvertTo-JsonText $o))
    [Console]::Out.Flush()
}

function Read-JsonAnswer {
    # -Json: ask with a "prompt" event, read one line from stdin. Only an explicit yes counts; end of input is no.
    param([string]$Kind, $Fields)
    $f = [ordered]@{ kind = $Kind; step = $script:CurrentStep }
    foreach ($k in $Fields.Keys) { $f[$k] = $Fields[$k] }
    Write-Event 'prompt' $f
    $line = $null
    try { $line = [Console]::In.ReadLine() } catch { $line = $null }
    return (Test-JsonAnswer $line)
}

function Test-JsonAnswer {
    param([string]$Line)
    if ($null -eq $Line) { return $false }
    $t = $Line.Trim()
    return ($t -match '^(?i)(y|yes|true|accept)$' -or $t -match '"answer"\s*:\s*(true|"yes")')
}

function Get-FoundDetails {
    # Detection text -> @{ version; path } for the "plan" event ("rustc 1.97.1", "JDK 22 at C:\jdk", "C:\Windows\...\vulkan-1.dll").
    param([string]$Found)
    $v = $null; $p = $null
    if ($Found) {
        $head = ($Found -split ' at ')[0]
        if ($head -notmatch '^([A-Za-z]:\\|all of|not needed|checkout |clone )') {
            $ver = Get-VersionFromText $head; if ($ver) { $v = $ver.ToString() } elseif ($head -match '^(?:JDK|java)\s+(\d+)') { $v = $Matches[1] }
        }
        if ($Found -match '(?:^|\s)([A-Za-z]:\\[^\r\n]*|/[^\s][^\r\n]*)$') { $p = $Matches[1].Trim() }
    }
    return @{ version = $v; path = $p }
}

function Get-FreeBytes {
    # Free space on the drive that holds $Path (or its nearest existing parent); $null when unknown.
    param([string]$Path)
    try {
        $p = $Path
        while ($p -and -not (Test-Path -LiteralPath $p)) { $p = Split-Path $p -Parent }
        if (-not $p) { return $null }
        $root = [IO.Path]::GetPathRoot((Resolve-Path -LiteralPath $p).ProviderPath)
        return [long](New-Object System.IO.DriveInfo($root)).AvailableFreeSpace
    } catch { return $null }
}

function Get-DownloadInfo {
    # packages.toml download entry -> fields of the "plan" event.
    param([string]$Name)
    $e = $script:Data["download.$Name"]
    $pinned = $e.Contains('sha256')
    return [ordered]@{
        name = $Name; url = $e['url']; size_mb = $(if ($e.Contains('size_mb')) { $e['size_mb'] } else { $null })
        sha256 = $(if ($pinned) { $e['sha256'] } else { $null }); sha256_url = $(if ($e.Contains('sha256_url')) { $e['sha256_url'] } else { $null })
        pinned = $pinned; pin_status = $(if ($e.Contains('pin_status')) { $e['pin_status'] } else { $null })
        cross_check = $(if ($e.Contains('index_key')) { $script:Data['zig']['index_url'] } else { $null })
    }
}

# editor.ron: "(\n    key: value,\n ...)" ; values escape '%' and ',' (crates/engine-editor/src/prefs.rs)
function ConvertTo-PrefValue { param([string]$V) return $V.Replace('%', '%25').Replace(',', '%2C').Replace("`n", '%0A') }

function Merge-Preferences {
    # Adds the tool_* keys that are missing or empty; keeps every non-empty value the user already has
    # (reported in .Kept) unless -Overwrite names the key. Returns @{ Text; Set; Kept }.
    param([string]$Text, $Values, [string[]]$Overwrite = @())
    $set = @(); $kept = @()
    if ([string]::IsNullOrWhiteSpace($Text)) { $Text = "(`n)`n" }
    $nl = "`n"; if ($Text.Contains("`r`n")) { $nl = "`r`n" }
    $lines = New-Object System.Collections.ArrayList
    foreach ($l in ($Text -split "`r?`n")) { [void]$lines.Add($l) }
    # drop the empty tail so that the closing ")" is the last non-empty line
    while ($lines.Count -gt 0 -and $lines[$lines.Count - 1].Trim() -eq '') { $lines.RemoveAt($lines.Count - 1) }
    $closeIndex = -1
    for ($i = $lines.Count - 1; $i -ge 0; $i--) { if ($lines[$i].Trim() -eq ')') { $closeIndex = $i; break } }
    if ($closeIndex -lt 0) { throw "editor.ron has no closing ')': refusing to edit it" }
    foreach ($key in $Values.Keys) {
        $value = ConvertTo-PrefValue ([string]$Values[$key])
        $found = -1
        for ($i = 0; $i -lt $closeIndex; $i++) { if ($lines[$i] -match ('^\s*' + [regex]::Escape($key) + '\s*:')) { $found = $i; break } }
        if ($found -ge 0) {
            $existing = ($lines[$found] -replace '^\s*[^:]+:\s*', '') -replace ',\s*$', ''
            if ($existing.Trim() -ne '' -and $existing.Trim() -ne $value -and ($Overwrite -notcontains $key)) { $kept += $key; continue }
            if ($existing.Trim() -eq $value) { continue }
            $lines[$found] = "    ${key}: $value,"
            $set += $key
        } else {
            $lines.Insert($closeIndex, "    ${key}: $value,")
            $closeIndex++
            $set += $key
        }
    }
    return @{ Text = (($lines -join $nl) + $nl); Set = $set; Kept = $kept }
}

# ---- state (resume) and manifest (uninstall) ----

function Get-StepDecision {
    # 'skip' | 'run'. -Mode: 'normal' (skip what is detected), 'resume' (also skip what the state file says is done),
    # 'repair' (ignore the state file, trust only detection).
    param([string]$Mode, $State, [string]$Id, [bool]$Detected)
    if ($Detected) { return 'skip' }
    return 'run'
}

function Get-ResumeSkip {
    param($State, [string]$Id, [bool]$Detected, [string]$Mode)
    if ($Mode -eq 'resume' -and $null -ne $State) {
        $entry = @($State.steps | Where-Object { $_.id -eq $Id }) | Select-Object -First 1
        if ($null -ne $entry -and $entry.status -eq 'done') { return $true }
    }
    return $Detected
}

function Set-StepState {
    param($State, [string]$Id, [string]$Status, [string]$ErrorText = '')
    $steps = @($State.steps | Where-Object { $_.id -ne $Id })
    $steps += [pscustomobject]@{ id = $Id; status = $Status; time = (Get-Date).ToString('s'); error = $ErrorText }
    $State.steps = $steps
    return $State
}

function Get-PendingSteps {
    # Ids of the plan that a resumed run still has to look at: everything not marked done.
    param($State, [string[]]$PlanIds)
    $done = @($State.steps | Where-Object { $_.status -eq 'done' } | ForEach-Object { $_.id })
    return [string[]]@($PlanIds | Where-Object { $done -notcontains $_ })
}

function Get-UninstallPlan {
    # Manifest entries -> actions, newest first. Only things the installer recorded as ADDED are listed;
    # packages that were present before the install were never recorded and are never touched.
    param($Manifest)
    $plan = @()
    $entries = @($Manifest.entries)
    [array]::Reverse($entries)
    foreach ($e in $entries) {
        switch ($e.kind) {
            'shortcut'    { $plan += [pscustomobject]@{ Action = 'delete-file'; Target = $e.detail; Ask = $false; Entry = $e } }
            'fileassoc'   { $plan += [pscustomobject]@{ Action = 'restore-assoc'; Target = $e.id; Ask = $false; Entry = $e } }
            'env'         { $plan += [pscustomobject]@{ Action = 'restore-env'; Target = $e.id; Ask = $false; Entry = $e } }
            'path-entry'  { $plan += [pscustomobject]@{ Action = 'remove-path'; Target = $e.detail; Ask = $false; Entry = $e } }
            'prefs'       { $plan += [pscustomobject]@{ Action = 'restore-prefs'; Target = $e.detail; Ask = $true; Entry = $e } }
            'cargo-config'{ $plan += [pscustomobject]@{ Action = 'remove-cargo-block'; Target = $e.detail; Ask = $false; Entry = $e } }
            'cargo-crate' { $plan += [pscustomobject]@{ Action = 'cargo-uninstall'; Target = $e.id; Ask = $false; Entry = $e } }
            'rust-target' { $plan += [pscustomobject]@{ Action = 'rustup-target-remove'; Target = $e.id; Ask = $false; Entry = $e } }
            'dir'         { $plan += [pscustomobject]@{ Action = 'delete-dir'; Target = $e.detail; Ask = $true; Entry = $e } }
            'rustup'      { $plan += [pscustomobject]@{ Action = 'rustup-self-uninstall'; Target = 'rustup'; Ask = $true; Entry = $e } }
            'winget'      { $plan += [pscustomobject]@{ Action = 'winget-uninstall'; Target = $e.id; Ask = $true; Entry = $e } }
            default       { }
        }
    }
    return $plan
}

# ----------------------------------------------------------------------------------------
# Environment, logging, native commands
# ----------------------------------------------------------------------------------------

function Get-Dirs {
    param([string]$InstallDirArg, [string]$CacheArg)
    $local = $env:LOCALAPPDATA; if (-not $local) { $local = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'AppData\Local' }
    $home_ = $env:USERPROFILE; if (-not $home_) { $home_ = [Environment]::GetFolderPath('UserProfile') }
    $appdata = $env:APPDATA; if (-not $appdata) { $appdata = Join-Path $home_ 'AppData\Roaming' }
    $name = $script:Data['project']['install_dir_name']
    $install = $InstallDirArg; if (-not $install) { $install = Join-Path $local $name }
    $stateDir = Join-Path (Join-Path $local $script:Data['project']['state_dir_name']) 'install'
    $cache_ = $CacheArg; if (-not $cache_) { $cache_ = Join-Path $install 'cache' }
    $cargoHome = $env:CARGO_HOME; if (-not $cargoHome) { $cargoHome = Join-Path $home_ '.cargo' }
    return @{ Local = $local; Home = $home_; AppData = $appdata; Install = $install; State = $stateDir; Cache = $cache_; CargoHome = $cargoHome
              StateFile = (Join-Path $stateDir 'state.json'); ManifestFile = (Join-Path $stateDir 'manifest.json'); Logs = (Join-Path $stateDir 'logs')
              Prefs = (Join-Path (Join-Path $appdata 'PiskaEngine') 'editor.ron') }
}

function Write-Info {
    param([string]$Text)
    if ($script:Json) { Write-Event 'log' ([ordered]@{ level = 'info'; step = $script:CurrentStep; text = $Text }) } else { Write-Host $Text }
    Add-LogLine $Text
}
function Write-Warn {
    param([string]$Text)
    if ($script:Json) { Write-Event 'log' ([ordered]@{ level = 'warn'; step = $script:CurrentStep; text = $Text }) } else { Write-Host "warning: $Text" -ForegroundColor Yellow }
    Add-LogLine "warning: $Text"
}
function Add-LogLine {
    param([string]$Text)
    if ($script:LogFile -and -not $script:Opt.DryRun) { try { Add-Content -LiteralPath $script:LogFile -Value ("{0} {1}" -f (Get-Date).ToString('s'), $Text) -Encoding UTF8 } catch { } }
}

function Confirm-Action {
    # -Key / -Subject identify the question for -Json (the graphical installer translates known keys).
    param([string]$Question, [bool]$DefaultYes = $true, [string]$Key = 'confirm', [string]$Subject = '')
    if ($script:Opt.Yes) { return $true }
    if ($script:Json) { return (Read-JsonAnswer -Kind 'confirm' -Fields ([ordered]@{ key = $Key; subject = $Subject; question = $Question; default = $DefaultYes })) }
    if (-not [Environment]::UserInteractive -or $Host.Name -eq 'Default Host') { return $false }
    $suffix = if ($DefaultYes) { '[Y/n]' } else { '[y/N]' }
    $answer = Read-Host "$Question $suffix"
    if ($answer -eq '') { return $DefaultYes }
    return $answer -match '^(y|yes|\u0434|\u0434\u0430)$'
}

function Confirm-License {
    # Licences are accepted only with -AcceptLicenses or an explicit answer; -Yes alone is not enough.
    param([string]$Name, [string]$Url, [string]$Id = '')
    if ($script:Opt.AcceptLicenses) { return $true }
    if ($Id -and (@($script:Opt['AcceptLicense']) -contains $Id)) { return $true }
    if ($script:Opt.Yes) { throw "'$Name' needs you to accept its licence ($Url). Re-run with -AcceptLicenses to accept it, or run without -Yes to be asked." }
    if ($script:Json) { return (Read-JsonAnswer -Kind 'licence' -Fields ([ordered]@{ licence = $Id; name = $Name; url = $Url })) }
    return (Confirm-Action "Do you accept the licence of $Name ($Url)?" $false)
}

function Test-IsAdmin {
    try { return ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) } catch { return $false }
}

function Find-Tool {
    param([string]$Name)
    if ($script:Opt.AssumeClean) { return $null }
    $cmd = Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd) { return $cmd.Source }
    return $null
}

function Test-Fact {
    # File-system probes go through here so -AssumeClean can pretend nothing exists.
    param([string]$Path, [string]$Type = 'Any')
    if ($script:Opt.AssumeClean) { return $false }
    return (Test-Path -LiteralPath $Path -PathType $Type)
}

function Get-ToolVersion {
    param([string]$Exe, [string[]]$Arguments)
    if (-not $Exe) { return $null }
    try {
        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        $text = (& $Exe @Arguments 2>&1 | Out-String)
        $ErrorActionPreference = $old
        return (Get-VersionFromText $text)
    } catch { return $null }
}

function Invoke-Native {
    param([string]$File, [string[]]$Arguments, [string]$WorkDir)
    Add-LogLine ("run: {0} {1}" -f $File, ($Arguments -join ' '))
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    if ($WorkDir) { Push-Location $WorkDir }
    try {
        & $File @Arguments 2>&1 | ForEach-Object {
            if ($script:Json) { Write-Event 'log' ([ordered]@{ level = 'output'; step = $script:CurrentStep; text = "$_" }) } else { Write-Host "$_" }
            Add-LogLine "$_"
        }
        $code = $LASTEXITCODE
    } finally { if ($WorkDir) { Pop-Location }; $ErrorActionPreference = $old }
    return $code
}

function Update-SessionPath {
    # Programs installed by earlier steps appear in the registry PATH, not in this process.
    $machine = [Environment]::GetEnvironmentVariable('Path', 'Machine'); $user = [Environment]::GetEnvironmentVariable('Path', 'User')
    $extra = @((Join-Path $script:Dirs.CargoHome 'bin'), (Join-Path $script:Dirs.Local 'Microsoft\WinGet\Links'))
    $env:Path = (@($machine, $user) + $extra | Where-Object { $_ }) -join ';'
}

function Get-Proxy {
    $p = $env:HTTPS_PROXY; if (-not $p) { $p = $env:https_proxy }
    return $p
}

function Invoke-Download {
    # Plain download to a file (TLS 1.2+, proxy aware). Never executes anything.
    param([string]$Url, [string]$OutFile)
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    if ($script:Json -and $script:CurrentStep) { Invoke-DownloadWithProgress -Url $Url -OutFile $OutFile; return }
    $params = @{ Uri = $Url; OutFile = $OutFile; UseBasicParsing = $true }
    $proxy = Get-Proxy; if ($proxy) { $params['Proxy'] = $proxy }
    $old = $ProgressPreference; $ProgressPreference = 'SilentlyContinue'
    try { Invoke-WebRequest @params } finally { $ProgressPreference = $old }
}

function Invoke-DownloadWithProgress {
    # -Json: the same plain download, read in blocks so that "progress" events (bytes of total) can be sent.
    param([string]$Url, [string]$OutFile, [int]$EveryMs = 250)
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $request = [System.Net.WebRequest]::Create($Url)
    $proxy = Get-Proxy; if ($proxy -and $request -is [System.Net.HttpWebRequest]) { $request.Proxy = New-Object System.Net.WebProxy($proxy) }
    $response = $request.GetResponse()
    try {
        $total = $response.ContentLength; if ($total -lt 0) { $total = $null }
        $in = $response.GetResponseStream(); $out = [IO.File]::Create($OutFile)
        try {
            $buffer = New-Object byte[] 65536; [long]$done = 0; $last = [DateTime]::MinValue
            while (($n = $in.Read($buffer, 0, $buffer.Length)) -gt 0) {
                $out.Write($buffer, 0, $n); $done += $n
                if (([DateTime]::UtcNow - $last).TotalMilliseconds -ge $EveryMs) {
                    Write-Event 'progress' ([ordered]@{ step = $script:CurrentStep; url = $Url; bytes = $done; total = $total }); $last = [DateTime]::UtcNow
                }
            }
            Write-Event 'progress' ([ordered]@{ step = $script:CurrentStep; url = $Url; bytes = $done; total = $total })
        } finally { $out.Dispose(); $in.Dispose() }
    } finally { $response.Close() }
}

function Get-RemoteText {
    param([string]$Url)
    $tmp = [IO.Path]::GetTempFileName()
    try { Invoke-Download -Url $Url -OutFile $tmp; return [IO.File]::ReadAllText($tmp) } finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
}

function Get-VerifiedFile {
    # Download (or reuse from the cache) and verify the SHA-256. Returns the path of a file that passed.
    # Expected hash: the pin in packages.toml, else the publisher's .sha256 file; zig is also cross-checked with index.json.
    param([string]$Name, $Entry)
    Assert-Download -Entry $Entry -Name $Name
    $file = Join-Path $script:Dirs.Cache ([IO.Path]::GetFileName(([Uri]$Entry['url']).AbsolutePath))
    $expected = $null
    if ($Entry.Contains('sha256')) { $expected = $Entry['sha256'] }
    if (Test-FileChecksum -Path $file -Expected $(if ($expected) { $expected } else { 'x' })) { Write-Info "  using cached $file (checksum ok)"; return $file }
    if (-not $expected) {
        $expected = Get-Sha256FromText (Get-RemoteText $Entry['sha256_url'])
        if (-not $expected) { throw "could not read a SHA-256 from $($Entry['sha256_url'])" }
        if (Test-FileChecksum -Path $file -Expected $expected) { Write-Info "  using cached $file (checksum ok)"; return $file }
    }
    if ($Entry.Contains('index_key')) {
        $fromIndex = Get-ZigIndexSha -Json (Get-RemoteText $script:Data['zig']['index_url']) -Version $script:Data['zig']['version'] -Key $Entry['index_key']
        if ($fromIndex -ne $expected) { throw "zig checksum pinned in packages.toml ($expected) differs from index.json ($fromIndex): refusing to download" }
    }
    New-Item -ItemType Directory -Force -Path $script:Dirs.Cache | Out-Null
    $part = "$file.part"
    Write-Info "  downloading $($Entry['url'])"
    Invoke-Download -Url $Entry['url'] -OutFile $part
    if (-not (Test-FileChecksum -Path $part -Expected $expected)) {
        $actual = Get-FileSha256 $part
        Remove-Item -LiteralPath $part -Force
        throw "checksum mismatch for $Name`: expected $expected, got $actual. The file was deleted and NOT used."
    }
    Move-Item -LiteralPath $part -Destination $file -Force
    Write-Info "  sha256 ok: $expected"
    return $file
}

function Describe-Download {
    param([string]$Name)
    $e = $script:Data["download.$Name"]
    $sum = if ($e.Contains('sha256')) { "sha256 $($e['sha256'])" } else { "sha256 from $($e['sha256_url'])" }
    $extra = if ($e.Contains('index_key')) { " (cross-checked with $($script:Data['zig']['index_url']))" } else { '' }
    return @("download $($e['url']) (~$($e['size_mb']) MB)", "verify   $sum$extra", "cache    $($script:Dirs.Cache)")
}

# ---- manifest / state files ----

function Read-JsonFile { param([string]$Path) if (Test-Path -LiteralPath $Path) { return (Get-Content -LiteralPath $Path -Raw -Encoding UTF8 | ConvertFrom-Json) } return $null }
function Write-JsonFile { param([string]$Path, $Object) New-Item -ItemType Directory -Force -Path (Split-Path $Path) | Out-Null; ConvertTo-Json -InputObject $Object -Depth 8 | Set-Content -LiteralPath $Path -Encoding UTF8 }

function Add-ManifestEntry {
    param([string]$Kind, [string]$Id, [string]$Detail = '', [string]$Previous = '', [string]$Component = '')
    $m = Read-JsonFile $script:Dirs.ManifestFile
    if ($null -eq $m) { $m = [pscustomobject]@{ version = 1; entries = @() } }
    $entries = @($m.entries) + [pscustomobject]@{ kind = $Kind; id = $Id; detail = $Detail; previous = $Previous; component = $Component; time = (Get-Date).ToString('s') }
    $m.entries = $entries
    Write-JsonFile $script:Dirs.ManifestFile $m
}

function Set-UserEnv {
    param([string]$Name, [string]$Value, [string]$Component)
    $old = [Environment]::GetEnvironmentVariable($Name, 'User')
    if ($old -eq $Value) { return }
    [Environment]::SetEnvironmentVariable($Name, $Value, 'User')
    Set-Item -Path "Env:$Name" -Value $Value
    Add-ManifestEntry -Kind 'env' -Id $Name -Detail $Value -Previous $(if ($null -eq $old) { '' } else { $old }) -Component $Component
    Write-Info "  set user environment variable $Name=$Value (new terminals see it)"
}

# ----------------------------------------------------------------------------------------
# Detection of the pieces (all read-only)
# ----------------------------------------------------------------------------------------

function Get-VsWhere {
    $pf = ${env:ProgramFiles(x86)}; if (-not $pf) { $pf = $env:ProgramFiles }
    if (-not $pf) { return $null }
    $p = Join-Path $pf 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Fact $p 'Leaf') { return $p }
    return $null
}

function Get-VisualStudioPath {
    $vswhere = Get-VsWhere
    if (-not $vswhere) { return $null }
    $out = & $vswhere -latest -products '*' -requires $script:Data['winget']['vs_required_component'] -property installationPath 2>$null
    return ($out | Where-Object { $_ } | Select-Object -First 1)
}

function Get-JdkHome {
    if ($script:Opt.AssumeClean) { return $null }
    if ($env:JAVA_HOME -and (Test-Path (Join-Path $env:JAVA_HOME 'bin\java.exe'))) { return $env:JAVA_HOME }
    $java = Find-Tool 'java'
    if (-not $java) { return $null }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $text = (& $java -XshowSettings:property -version 2>&1 | Out-String)
    $ErrorActionPreference = $old
    if ($text -match 'java\.home\s*=\s*(.+)') { return $Matches[1].Trim() }
    return $null
}

function Get-JdkMajor {
    param([string]$JdkHome)
    if (-not $JdkHome) { return 0 }
    $exe = Join-Path $JdkHome 'bin\java.exe'
    if (-not (Test-Path $exe)) { return 0 }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $text = (& $exe -version 2>&1 | Out-String)
    $ErrorActionPreference = $old
    if ($text -match 'version "(\d+)(?:\.(\d+))?') { if ($Matches[1] -eq '1' -and $Matches[2]) { return [int]$Matches[2] } return [int]$Matches[1] }
    return 0
}

function Get-SdkRoot {
    if ($script:Opt.AndroidSdk) { return $script:Opt.AndroidSdk }
    foreach ($v in @($env:ANDROID_HOME, $env:ANDROID_SDK_ROOT)) { if ($v -and (Test-Fact $v 'Container') -and -not $script:Opt.AssumeClean) { return $v } }
    foreach ($p in @((Join-Path $script:Dirs.Local 'Android\Sdk'), (Join-Path $script:Dirs.Home 'Android\Sdk'))) {
        if ((Test-Fact (Join-Path $p 'platforms') 'Container') -or (Test-Fact (Join-Path $p 'cmdline-tools') 'Container') -or (Test-Fact (Join-Path $p 'platform-tools') 'Container')) { return $p }
    }
    return (Join-Path $script:Dirs.Local 'Android\Sdk')
}

function Get-RustTargetInstalled {
    param([string]$Target)
    $rustc = Find-Tool 'rustc'
    if (-not $rustc) { return $false }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    $root = ((& $rustc --print sysroot 2>$null) | Select-Object -First 1)
    $ErrorActionPreference = $old
    return [bool]($root -and (Test-Path (Join-Path $root "lib\rustlib\$Target")))
}

# ----------------------------------------------------------------------------------------
# Steps
# ----------------------------------------------------------------------------------------

function Test-SdkPackage {
    # Is an sdkmanager package present? Newer platform / build-tools / NDK also count: piska-export uses the newest installed.
    param([string]$Sdk, [string]$Package, $Min)
    $kind = ($Package -split ';')[0]
    switch ($kind) {
        'platform-tools' { return (Test-Fact (Join-Path $Sdk 'platform-tools') 'Container') }
        'platforms' {
            foreach ($d_ in (Get-ChildItem (Join-Path $Sdk 'platforms') -Directory -ErrorAction SilentlyContinue)) {
                if ($script:Opt.AssumeClean) { break }
                if ($d_.Name -match '^android-(\d+)' -and [int]$Matches[1] -ge [int]$Min['android_api'] -and (Test-Path (Join-Path $d_.FullName 'android.jar'))) { return $true }
            }
            return $false
        }
        'build-tools' {
            foreach ($d_ in (Get-ChildItem (Join-Path $Sdk 'build-tools') -Directory -ErrorAction SilentlyContinue)) {
                if ($script:Opt.AssumeClean) { break }
                if ((Test-Path (Join-Path $d_.FullName 'aapt2.exe')) -and (Test-Path (Join-Path $d_.FullName 'zipalign.exe'))) { return $true }
            }
            return $false
        }
        'ndk' {
            foreach ($d_ in (Get-ChildItem (Join-Path $Sdk 'ndk') -Directory -ErrorAction SilentlyContinue)) {
                if ($script:Opt.AssumeClean) { break }
                if ((Test-Path (Join-Path $d_.FullName 'source.properties')) -and ($d_.Name -split '\.')[0] -match '^\d+$' -and [int]($d_.Name -split '\.')[0] -ge [int]$Min['ndk_major']) { return $true }
            }
            return $false
        }
        default { return $false }
    }
}

function Get-ExePath {
    # Where a release build puts a program: CARGO_TARGET_DIR if set, else the workspace's target\ (the modeler is its own workspace in modeler\).
    param([string]$Repo, [string]$Name)
    if ($env:CARGO_TARGET_DIR) { return (Join-Path $env:CARGO_TARGET_DIR "release\$Name.exe") }
    if ($Name -eq 'piska-modeler') { return (Join-Path $Repo "modeler\target\release\$Name.exe") }
    return (Join-Path $Repo "target\release\$Name.exe")
}

function Get-Ctx {
    # Values shared by the steps' Detect / Run blocks (blocks are not closures: Invoke-Block sets these as locals).
    $d = $script:Data; $dirs = $script:Dirs; $a = $d['android']
    $ctx = @{ d = $d; dirs = $dirs; min = $d['min']; a = $a; sdk = (Get-SdkRoot); repoDir = $script:RepoDir; Selected = $script:SelectedNow }
    $ctx.zigVersion = $d['zig']['version']
    $ctx.zigRoot = Join-Path $dirs.Install "tools\zig\$($ctx.zigVersion)"
    $ctx.zigExe = Join-Path $ctx.zigRoot "zig-x86_64-windows-$($ctx.zigVersion)\zig.exe"
    $ctx.cargoExe = Join-Path $dirs.CargoHome 'bin\cargo.exe'
    $ctx.vsOverride = '--passive --wait --norestart --nocache ' + ((@($d['winget']['vs_workloads']) | ForEach-Object { "--add $_" }) -join ' ') + ' --includeRecommended'
    $ctx.rustupEntry = $d['download.rustup_windows']
    $ctx.cmd = $d['download.android_cmdline_windows']
    $ctx.jdkMin = [int]$d['min']['java']
    $ctx.sdkManager = Join-Path $ctx.sdk 'cmdline-tools\latest\bin\sdkmanager.bat'
    $ctx.packages = @($a['sdk_packages'])
    $ctx.pkgDirs = @{ 'platform-tools' = 'platform-tools'; "platforms;$($a['platform'])" = "platforms\$($a['platform'])"; "build-tools;$($a['build_tools'])" = "build-tools\$($a['build_tools'])"; "ndk;$($a['ndk'])" = "ndk\$($a['ndk'])" }
    $ctx.pkgQuoted = (@($ctx.packages | ForEach-Object { '"' + $_ + '"' })) -join ' '
    $ctx.missing = { @($packages | Where-Object { -not (Test-SdkPackage -Sdk $sdk -Package $_ -Min $min) }) }
    $ctx.ndkDir = Join-Path $ctx.sdk "ndk\$($a['ndk'])"
    $ctx.zigEntry = $d['download.zig_windows_x86_64']
    $ctx.lt = $d['rust']['linux_target']
    $ctx.repo = $d['project']['repo']; $ctx.branch = $d['project']['branch']; $ctx.bins = @($d['project']['binaries']); $ctx.modelerManifest = $d['project']['modeler_manifest']; $ctx.modelerBin = $d['project']['modeler_binary']
    $ctx.programs = Join-Path $dirs.AppData 'Microsoft\Windows\Start Menu\Programs\Piska Engine'
    $ctx.icon = Join-Path $ctx.repoDir 'assets\branding\icon.ico'
    $ctx.apps = @(@('Piska Hub', 'piska-hub'), @('Piska Editor', 'piska-editor'), @('Piska Modeler', 'piska-modeler'))
    $ctx.uninstaller = Join-Path $dirs.Install 'uninstall.ps1'
    return $ctx
}

function Invoke-Block {
    param([scriptblock]$Block, [hashtable]$BlockVars)
    # Dynamic scope: the block, called from here, sees these locals.
    foreach ($key in $script:Ctx.Keys) { Set-Variable -Name $key -Value $script:Ctx[$key] }
    foreach ($key in $BlockVars.Keys) { Set-Variable -Name $key -Value $BlockVars[$key] }
    & $Block
}

function New-Step {
    param([string]$Id, [string]$Component, [string]$Title, [scriptblock]$Detect, [string[]]$Plan, [scriptblock]$Run, [string]$Elevation = '', [string]$Licence = '', [string]$LicenceUrl = '', [hashtable]$Vars = @{},
        [string]$LicenceId = '', [string[]]$Downloads = @())
    return [pscustomobject]@{ Vars = $Vars; Id = $Id; Component = $Component; Title = $Title; Detect = $Detect; Plan = $Plan; Run = $Run; Elevation = $Elevation; Licence = $Licence; LicenceUrl = $LicenceUrl
                              LicenceId = $LicenceId; Downloads = $Downloads }
}

function Invoke-WingetInstall {
    param([string]$PackageId, [string]$Override = '', [string]$Component)
    $winget = Find-Tool 'winget'
    if (-not $winget) { throw "winget was not found. Install 'App Installer' from the Microsoft Store (or update Windows), or install $PackageId by hand; see docs/install.md." }
    $args_ = @('install', '--id', $PackageId, '--exact', '--source', 'winget', '--accept-source-agreements', '--accept-package-agreements', '--silent')
    if ($Override) { $args_ += @('--override', $Override) }
    $code = Invoke-Native -File $winget -Arguments $args_
    if ($code -eq 3010) { $script:RebootNeeded = $true }
    elseif ($code -eq 1223 -or $code -eq 1602) {
        # ERROR_CANCELLED / ERROR_INSTALL_USEREXIT: the UAC prompt was answered "No" or the installer was cancelled.
        $script:ElevationRefused = $true
        throw "the administrator rights prompt for $PackageId was refused or the installer was cancelled (exit code $code). Run again and answer Yes, or install $PackageId by hand."
    }
    elseif ($code -ne 0 -and $code -ne -1978335189 -and $code -ne -1978335135) { throw "winget install $PackageId failed with exit code $code" }
    Add-ManifestEntry -Kind 'winget' -Id $PackageId -Component $Component
    Update-SessionPath
}

function Get-WingetPlanLine {
    param([string]$PackageId, [string]$Override = '')
    $line = "run      winget install --id $PackageId --exact --source winget --accept-source-agreements --accept-package-agreements --silent"
    if ($Override) { $line += " --override `"$Override`"" }
    return $line
}

function Get-Steps {
    param([string[]]$Selected)
    # Detect / Run blocks of the steps see the same variables (Invoke-Block sets them from the context).
    $script:Ctx = Get-Ctx
    foreach ($k in $script:Ctx.Keys) { Set-Variable -Name $k -Value $script:Ctx[$k] }
    $steps = New-Object System.Collections.ArrayList

    if ($Selected -contains 'core') {
        [void]$steps.Add((New-Step 'core.git' 'core' 'Git' {
                    $v = Get-ToolVersion (Find-Tool 'git') @('--version'); if (Test-VersionAtLeast $v $min['git']) { "git $v" } else { $null }
                } @((Get-WingetPlanLine $d['winget']['git'])) {
                    Invoke-WingetInstall -PackageId $d['winget']['git'] -Component 'core' } -Elevation 'The Git installer may ask Windows for administrator rights (one UAC prompt).'))

        [void]$steps.Add((New-Step 'core.buildtools' 'core' 'Visual Studio Build Tools (C++ workload + Windows SDK)' {
                    $p = Get-VisualStudioPath; if ($p) { $p } else { $null }
                } @((Get-WingetPlanLine $d['winget']['buildtools'] $vsOverride), 'note     about 5 GB; a reboot may be requested afterwards (the installer then stops: re-run with -Resume)') {
                    Invoke-WingetInstall -PackageId $d['winget']['buildtools'] -Override $vsOverride -Component 'core'
                    if (-not (Get-VisualStudioPath) -and -not $script:RebootNeeded) { throw 'Build Tools were installed but vswhere does not see the C++ workload; open "Visual Studio Installer" and add "Desktop development with C++"' }
                } -Elevation 'Visual Studio Build Tools install system-wide: Windows will show ONE administrator (UAC) prompt for it.' -Licence 'Visual Studio Build Tools' -LicenceUrl 'https://visualstudio.microsoft.com/license-terms/' -LicenceId 'vs-buildtools'))

        $rustupPlan = @(Describe-Download 'rustup_windows') + @("run      rustup-init.exe -y --default-host $($d['rust']['windows_host']) --default-toolchain $($d['rust']['toolchain']) --profile minimal")
        [void]$steps.Add((New-Step 'core.rustup' 'core' 'Rust (rustup, stable, MSVC)' {
                    $rustc = Find-Tool 'rustc'; if (-not $rustc) { $rustc = (Join-Path $dirs.CargoHome 'bin\rustc.exe'); if (-not (Test-Fact $rustc 'Leaf')) { $rustc = $null } }
                    $v = Get-ToolVersion $rustc @('--version'); if (Test-VersionAtLeast $v $min['rust']) { "rustc $v" } else { $null }
                } $rustupPlan {
                    $existing = Find-Tool 'rustup'
                    if ($existing) {
                        $code = Invoke-Native -File $existing -Arguments @('update', $d['rust']['toolchain']); if ($code -ne 0) { throw "rustup update failed ($code)" }
                    } else {
                        $file = Get-VerifiedFile 'rustup_windows' $rustupEntry
                        $code = Invoke-Native -File $file -Arguments @('-y', '--default-host', $d['rust']['windows_host'], '--default-toolchain', $d['rust']['toolchain'], '--profile', 'minimal')
                        if ($code -ne 0) { throw "rustup-init failed ($code)" }
                        Add-ManifestEntry -Kind 'rustup' -Id 'rustup' -Component 'core'
                    }
                    Update-SessionPath
                } -Downloads @('rustup_windows')))

        [void]$steps.Add((New-Step 'core.rust-components' 'core' 'Rust components (clippy, rustfmt)' {
                    $rustup = Find-Tool 'rustup'; if (-not $rustup) { return $null }
                    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'; $list = (& $rustup component list --installed 2>&1 | Out-String); $ErrorActionPreference = $old
                    if ($list -match 'clippy' -and $list -match 'rustfmt') { 'installed' } else { $null }
                } @("run      rustup component add $(@($d['rust']['components']) -join ' ')") {
                    $code = Invoke-Native -File (Find-Tool 'rustup') -Arguments (@('component', 'add') + @($d['rust']['components'])); if ($code -ne 0) { throw "rustup component add failed ($code)" }
                }))

        [void]$steps.Add((New-Step 'core.vulkan' 'core' 'Vulkan runtime (GPU driver)' {
                    $dll = Join-Path ($(if ($env:SystemRoot) { $env:SystemRoot } else { 'C:\Windows' })) 'System32\vulkan-1.dll'
                    if (Test-Fact $dll 'Leaf') { $dll } else { $null }
                } @('note     vulkan-1.dll is installed by your GPU driver. If it is missing, install the current driver from NVIDIA / AMD / Intel (the engine also runs on DX12).') {
                    Write-Warn 'vulkan-1.dll was not found. Install the current driver from your GPU vendor; this installer does not install drivers.'
                }))
    }

    if ($Selected -contains 'cpp') {
        [void]$steps.Add((New-Step 'cpp.cmake' 'cpp' 'CMake' {
                    $v = Get-ToolVersion (Find-Tool 'cmake') @('--version'); if (Test-VersionAtLeast $v $min['cmake']) { "cmake $v" } else { $null }
                } @((Get-WingetPlanLine $d['winget']['cmake'])) { Invoke-WingetInstall -PackageId $d['winget']['cmake'] -Component 'cpp' } -Elevation 'The CMake installer may ask for administrator rights (one UAC prompt).'))
        [void]$steps.Add((New-Step 'cpp.ninja' 'cpp' 'Ninja' {
                    $v = Get-ToolVersion (Find-Tool 'ninja') @('--version'); if (Test-VersionAtLeast $v $min['ninja']) { "ninja $v" } else { $null }
                } @((Get-WingetPlanLine $d['winget']['ninja'])) { Invoke-WingetInstall -PackageId $d['winget']['ninja'] -Component 'cpp' }))
    }

    if ($Selected -contains 'web') {
        $t = $d['rust']['web_target']
        [void]$steps.Add((New-Step -Vars @{ t = $t } 'web.target' 'web' "Rust target $t" { if (Get-RustTargetInstalled $t) { $t } else { $null } } @("run      rustup target add $t") {
                    $code = Invoke-Native -File (Find-Tool 'rustup') -Arguments @('target', 'add', $t); if ($code -ne 0) { throw "rustup target add $t failed ($code)" }
                    Add-ManifestEntry -Kind 'rust-target' -Id $t -Component 'web'
                }))
    }

    if ($Selected -contains 'android') {
        [void]$steps.Add((New-Step 'android.jdk' 'android' "JDK $($a['jdk'])" {
                    $home_ = Get-JdkHome; $m = Get-JdkMajor $home_; if ($m -ge $jdkMin) { "JDK $m at $home_" } else { $null }
                } @((Get-WingetPlanLine $d['winget']['jdk'])) { Invoke-WingetInstall -PackageId $d['winget']['jdk'] -Component 'android' } -Elevation 'The JDK installer may ask for administrator rights (one UAC prompt).'))

        [void]$steps.Add((New-Step 'android.cmdline-tools' 'android' 'Android command-line tools (sdkmanager)' {
                    if (Test-Fact $sdkManager 'Leaf') { $sdkManager } elseif (-not $script:Opt.AssumeClean -and @(& $missing).Count -eq 0) { 'not needed: every SDK package is already installed' } else { $null }
                } (@(Describe-Download 'android_cmdline_windows') + @("extract  to $(Join-Path $sdk 'cmdline-tools\latest')")) {
                    $zip = Get-VerifiedFile 'android_cmdline_windows' $cmd
                    $tmp = Join-Path $dirs.Cache 'cmdline-tools-extract'
                    if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
                    Expand-Archive -LiteralPath $zip -DestinationPath $tmp -Force
                    $latest = Join-Path $sdk 'cmdline-tools\latest'
                    New-Item -ItemType Directory -Force -Path (Split-Path $latest) | Out-Null
                    Move-Item -LiteralPath (Join-Path $tmp 'cmdline-tools') -Destination $latest
                    Remove-Item -Recurse -Force $tmp
                    Add-ManifestEntry -Kind 'dir' -Id 'android-cmdline-tools' -Detail $latest -Component 'android'
                } -Downloads @('android_cmdline_windows')))

        [void]$steps.Add((New-Step 'android.licenses' 'android' 'Android SDK licences' {
                    if (Test-Fact (Join-Path $sdk 'licenses\android-sdk-license') 'Leaf') { 'accepted' } else { $null }
                } @("run      sdkmanager --sdk_root=$sdk --licenses   (answers 'y' to every licence; only after you accept)") {
                    $env:JAVA_HOME = $(if (Get-JdkHome) { Get-JdkHome } else { $env:JAVA_HOME })
                    $answers = ('y' * 40).ToCharArray() -join "`n"
                    $psi = New-Object System.Diagnostics.ProcessStartInfo
                    $psi.FileName = $env:ComSpec; $psi.Arguments = "/c `"`"$sdkManager`" --sdk_root=`"$sdk`" --licenses`""
                    $psi.RedirectStandardInput = $true; $psi.UseShellExecute = $false
                    $p = [Diagnostics.Process]::Start($psi); $p.StandardInput.Write($answers + "`n"); $p.StandardInput.Close(); $p.WaitForExit()
                    if ($p.ExitCode -ne 0) { throw "sdkmanager --licenses failed ($($p.ExitCode))" }
                } -Licence 'the Android SDK' -LicenceUrl 'https://developer.android.com/studio/terms' -LicenceId 'android-sdk'))

        [void]$steps.Add((New-Step 'android.packages' 'android' 'Android SDK packages' {
                    if (@(& $missing).Count -eq 0) { "all of: $($packages -join ', ')" } else { $null }
                } @("run      sdkmanager --sdk_root=$sdk $pkgQuoted") {
                    $env:JAVA_HOME = $(if (Get-JdkHome) { Get-JdkHome } else { $env:JAVA_HOME })
                    $todo = @(& $missing)
                    $code = Invoke-Native -File $sdkManager -Arguments (@("--sdk_root=$sdk") + $todo); if ($code -ne 0) { throw "sdkmanager failed ($code)" }
                    foreach ($p_ in $todo) { Add-ManifestEntry -Kind 'dir' -Id "sdk:$p_" -Detail (Join-Path $sdk $pkgDirs[$p_]) -Component 'android' }
                }))

        [void]$steps.Add((New-Step 'android.cargo-ndk' 'android' "cargo-ndk $($a['cargo_ndk'])" {
                    $cargo = Find-Tool 'cargo'; if (-not $cargo) { return $null }
                    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'; $text = (& $cargo ndk --version 2>&1 | Out-String); $ErrorActionPreference = $old
                    $v = Get-VersionFromText $text; if (Test-VersionAtLeast $v $a['cargo_ndk']) { "cargo-ndk $v" } else { $null }
                } @("run      cargo install cargo-ndk --version $($a['cargo_ndk']) --locked   (compiles from crates.io, a few minutes; cargo checks crate hashes)") {
                    $code = Invoke-Native -File (Find-Tool 'cargo') -Arguments @('install', 'cargo-ndk', '--version', $a['cargo_ndk'], '--locked'); if ($code -ne 0) { throw "cargo install cargo-ndk failed ($code)" }
                    Add-ManifestEntry -Kind 'cargo-crate' -Id 'cargo-ndk' -Component 'android'
                }))

        foreach ($t in @($d['rust']['android_targets'])) {
            [void]$steps.Add((New-Step -Vars @{ t = $t } "android.target.$t" 'android' "Rust target $t" { if (Get-RustTargetInstalled $t) { $t } else { $null } } @("run      rustup target add $t") {
                        $code = Invoke-Native -File (Find-Tool 'rustup') -Arguments @('target', 'add', $t); if ($code -ne 0) { throw "rustup target add $t failed ($code)" }
                        Add-ManifestEntry -Kind 'rust-target' -Id $t -Component 'android'
                    }))
        }

        [void]$steps.Add((New-Step 'android.env' 'android' 'Environment: ANDROID_HOME, ANDROID_NDK_HOME (and JAVA_HOME if unset)' {
                    if ($env:ANDROID_HOME -eq $sdk -and $env:ANDROID_NDK_HOME -and -not $script:Opt.AssumeClean) { 'set' } else { $null }
                } @("env      ANDROID_HOME=$sdk (user)", "env      ANDROID_NDK_HOME=$ndkDir (user)", 'env      JAVA_HOME=<JDK folder> (user, only if it is not set yet)') {
                    Set-UserEnv 'ANDROID_HOME' $sdk 'android'
                    $ndkNow = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
                    $target = if (Test-Path $ndkDir) { $ndkDir } elseif ($ndkNow) { $ndkNow.FullName } else { $ndkDir }
                    Set-UserEnv 'ANDROID_NDK_HOME' $target 'android'
                    if (-not [Environment]::GetEnvironmentVariable('JAVA_HOME', 'User') -and -not $env:JAVA_HOME) { $j = Get-JdkHome; if ($j) { Set-UserEnv 'JAVA_HOME' $j 'android' } }
                }))
    }

    if ($Selected -contains 'linux-cross') {
        [void]$steps.Add((New-Step 'linux-cross.zig' 'linux-cross' "zig $zigVersion" {
                    $v = Get-ToolVersion $(if ($env:PISKA_ZIG -and -not $script:Opt.AssumeClean) { $env:PISKA_ZIG } else { Find-Tool 'zig' }) @('version'); if (Test-VersionAtLeast $v $min['zig']) { "zig $v" } else { $null }
                } (@(Describe-Download 'zig_windows_x86_64') + @("extract  to $zigRoot")) {
                    $zip = Get-VerifiedFile 'zig_windows_x86_64' $zigEntry
                    New-Item -ItemType Directory -Force -Path $zigRoot | Out-Null
                    Expand-Archive -LiteralPath $zip -DestinationPath $zigRoot -Force
                    if (-not (Test-Path $zigExe)) { throw "zig.exe was not found in $zigRoot after extraction" }
                    Add-ManifestEntry -Kind 'dir' -Id 'zig' -Detail $zigRoot -Component 'linux-cross'
                } -Downloads @('zig_windows_x86_64')))
        [void]$steps.Add((New-Step 'linux-cross.target' 'linux-cross' "Rust target $lt" { if (Get-RustTargetInstalled $lt) { $lt } else { $null } } @("run      rustup target add $lt") {
                    $code = Invoke-Native -File (Find-Tool 'rustup') -Arguments @('target', 'add', $lt); if ($code -ne 0) { throw "rustup target add $lt failed ($code)" }
                    Add-ManifestEntry -Kind 'rust-target' -Id $lt -Component 'linux-cross'
                }))
        [void]$steps.Add((New-Step 'linux-cross.env' 'linux-cross' 'Environment: PISKA_ZIG' {
                    if ($env:PISKA_ZIG -and (Test-Fact $env:PISKA_ZIG 'Leaf')) { 'set' } else { $null }
                } @("env      PISKA_ZIG=$zigExe (user)") { Set-UserEnv 'PISKA_ZIG' $zigExe 'linux-cross' }))
    }

    if ($Selected -contains 'engine') {
        $cloneNote = if ($script:InCheckout) { "run      git -C `"$repoDir`" submodule update --init --recursive   (the modeler is a submodule in modeler\; skipped when it is already there)" } else { "run      git clone --recurse-submodules --branch $branch $repo `"$repoDir`"   (an existing clone: git pull --ff-only, then git submodule update --init --recursive)" }
        [void]$steps.Add((New-Step 'engine.source' 'engine' 'Engine sources (with the modeler submodule)' {
                    if ($script:InCheckout) { if (Test-Fact (Join-Path $repoDir "$($modelerManifest -replace '/', '\')") 'Leaf') { return "checkout $repoDir" } else { return $null } }
                    if (Test-Fact (Join-Path $repoDir '.git') 'Container') {
                        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
                        $dirty = (& git -C $repoDir status --porcelain 2>&1 | Out-String).Trim()
                        $ErrorActionPreference = $old
                        if ($dirty) { return "clone at $repoDir has local changes (left untouched)" }
                        return $null
                    }
                    return $null
                } @($cloneNote) {
                    $git = Find-Tool 'git'
                    if (-not $script:InCheckout) {
                        if (Test-Path (Join-Path $repoDir '.git')) {
                            $code = Invoke-Native -File $git -Arguments @('-C', $repoDir, 'pull', '--ff-only'); if ($code -ne 0) { throw "git pull failed ($code): resolve it by hand in $repoDir" }
                        } else {
                            New-Item -ItemType Directory -Force -Path (Split-Path $repoDir) | Out-Null
                            $code = Invoke-Native -File $git -Arguments @('clone', '--recurse-submodules', '--branch', $branch, $repo, $repoDir); if ($code -ne 0) { throw "git clone failed ($code). The repository may be private: sign in to GitHub (git credential manager / gh auth login) and run again." }
                            Add-ManifestEntry -Kind 'dir' -Id 'engine-source' -Detail $repoDir -Component 'engine'
                        }
                    }
                    $code = Invoke-Native -File $git -Arguments @('-C', $repoDir, 'submodule', 'update', '--init', '--recursive'); if ($code -ne 0) { throw "git submodule update failed ($code)" }
                }))
        $pkgArgs = ($bins | ForEach-Object { "-p $_" }) -join ' '
        [void]$steps.Add((New-Step 'engine.build' 'engine' 'Build hub, editor, export tool and modeler (release)' {
                    $ok = $true; foreach ($b in (@($bins) + $modelerBin)) { if (-not (Test-Fact (Get-ExePath $repoDir $b) 'Leaf')) { $ok = $false } }
                    if ($ok) { 'built' } else { $null }
                } @("run      cargo build --release $pkgArgs   (in $repoDir; 20-40 minutes, about 3 GB)", "run      cargo build --release --manifest-path $modelerManifest -p $modelerBin   (in $repoDir)") {
                    $cargo = Find-Tool 'cargo'
                    $code = Invoke-Native -File $cargo -Arguments (@('build', '--release') + ($bins | ForEach-Object { @('-p', $_) })) -WorkDir $repoDir
                    if ($code -ne 0) { throw "cargo build failed ($code). If the path is too long for Windows, install to a shorter -InstallDir." }
                    $code = Invoke-Native -File $cargo -Arguments @('build', '--release', '--manifest-path', $modelerManifest, '-p', $modelerBin) -WorkDir $repoDir
                    if ($code -ne 0) { throw "modeler build failed ($code)" }
                }))
        if (-not $script:Opt.NoShortcuts) {
            $plan = @($apps | ForEach-Object { "shortcut $(Join-Path $programs ($_[0] + '.lnk')) -> $(Get-ExePath $repoDir $_[1])  (icon $icon)" })
            if ($script:Opt.DesktopShortcut) { $plan += "shortcut desktop: Piska Hub.lnk" }
            [void]$steps.Add((New-Step 'engine.shortcuts' 'engine' 'Start-menu shortcuts' {
                        if (Test-Fact (Join-Path $programs 'Piska Hub.lnk') 'Leaf') { 'present' } else { $null }
                    } $plan {
                        New-Item -ItemType Directory -Force -Path $programs | Out-Null
                        $shell = New-Object -ComObject WScript.Shell
                        $targets = @($apps | ForEach-Object { , @((Join-Path $programs ($_[0] + '.lnk')), $_[1]) })
                        if ($script:Opt.DesktopShortcut) { $targets += , @((Join-Path ([Environment]::GetFolderPath('Desktop')) 'Piska Hub.lnk'), 'piska-hub') }
                        foreach ($t in $targets) {
                            $link = $shell.CreateShortcut($t[0]); $link.TargetPath = (Get-ExePath $repoDir $t[1]); $link.WorkingDirectory = $repoDir
                            if (Test-Path $icon) { $link.IconLocation = $icon }
                            $link.Save(); Add-ManifestEntry -Kind 'shortcut' -Id $t[1] -Detail $t[0] -Component 'engine'
                        }
                    }))
        }
        if ($script:Opt.FileAssociations -or (-not $script:Opt.Yes -and -not $script:Opt.DryRun)) {
            # The project file (.<project_extension>) opens the hub, which shows its Import page for it.
            $projExt = $d['project']['project_extension']
            $assocs = @(@('.pmodel', 'piska-modeler', 'Piska model'), @('.scene', 'piska-editor', 'Piska scene'), @('.scenario', 'piska-editor', 'Piska scenario'), @(".$projExt", 'piska-hub', $d['project']['project_file_description']))
            [void]$steps.Add((New-Step 'engine.assoc' 'engine' "File associations (.pmodel, .scene, .scenario, .$projExt; current user only)" {
                        if (-not $script:Opt.FileAssociations) { if (-not (Confirm-Action "Register file associations for .pmodel / .scene / .scenario / .$projExt (current user only)?" $false -Key 'file-associations')) { return 'declined' } }
                        $null
                    } @($assocs | ForEach-Object { "registry HKCU\Software\Classes\$($_[0]) -> PiskaEngine$($_[0])  (opens with $($_[1]).exe)" }) {
                        foreach ($as in $assocs) {
                            $ext = $as[0]; $progId = "PiskaEngine$ext"
                            $prev = (Get-ItemProperty -Path "HKCU:\Software\Classes\$ext" -ErrorAction SilentlyContinue).'(default)'
                            New-Item -Path "HKCU:\Software\Classes\$ext" -Force | Out-Null; Set-ItemProperty -Path "HKCU:\Software\Classes\$ext" -Name '(default)' -Value $progId
                            New-Item -Path "HKCU:\Software\Classes\$progId\shell\open\command" -Force | Out-Null
                            Set-ItemProperty -Path "HKCU:\Software\Classes\$progId" -Name '(default)' -Value $as[2]
                            Set-ItemProperty -Path "HKCU:\Software\Classes\$progId\shell\open\command" -Name '(default)' -Value ('"' + (Get-ExePath $repoDir $as[1]) + '" "%1"')
                            Add-ManifestEntry -Kind 'fileassoc' -Id $ext -Detail $progId -Previous $(if ($prev) { $prev } else { '' }) -Component 'engine'
                        }
                    } -Vars @{ assocs = $assocs; projExt = $projExt }))
        }
        [void]$steps.Add((New-Step 'engine.uninstaller' 'engine' 'Uninstall script' {
                    if (Test-Fact $uninstaller 'Leaf') { 'present' } else { $null }
                } @("copy     install.ps1 + packages.toml to $($dirs.Install)  (uninstall.ps1 removes only what the manifest lists)") {
                    New-Item -ItemType Directory -Force -Path $dirs.Install | Out-Null
                    Copy-Item -LiteralPath $script:SelfPath -Destination (Join-Path $dirs.Install 'install.ps1') -Force
                    Copy-Item -LiteralPath $script:DataPath -Destination (Join-Path $dirs.Install 'packages.toml') -Force
                    Set-Content -LiteralPath $uninstaller -Encoding UTF8 -Value "& (Join-Path `$PSScriptRoot 'install.ps1') -Uninstall @args"
                    Add-ManifestEntry -Kind 'dir' -Id 'installer-copy' -Detail $dirs.Install -Component 'engine'
                }))
    }

    # Configuration: the editor's Build & Tools page and the optional cargo config.
    if (-not $script:Opt.NoPrefs) {
        $prefsValues = Get-PrefsValues -Selected $Selected -Sdk $sdk -ZigExe $zigExe -CargoExe $cargoExe
        $plan = @($prefsValues.Keys | ForEach-Object { "prefs    $_ = $($prefsValues[$_])" })
        $plan += "file     $($dirs.Prefs)  (merged: existing values are kept, a backup .piska-backup-<time> is made first)"
        [void]$steps.Add((New-Step 'config.preferences' 'config' 'Editor preferences: Build & Tools paths' { $null } $plan {
                    $values = Get-PrefsValues -Selected $Selected -Sdk $sdk -ZigExe $zigExe -CargoExe $cargoExe -Actual
                    Update-PreferencesFile -File $dirs.Prefs -Values $values
                }))
    }
    if ($CargoTargetDir) {
        $cfg = Join-Path $dirs.CargoHome 'config.toml'
        $block = "[build]`ntarget-dir = `"$($CargoTargetDir.Replace('\','/'))`""
        [void]$steps.Add((New-Step 'config.cargo' 'config' 'Cargo config: shared target dir' {
                    if ((Test-Fact $cfg 'Leaf') -and ((Get-Content -LiteralPath $cfg -Raw) -match 'piska-engine installer: target-dir')) { 'present' } else { $null }
                } @("file     $cfg  (a delimited block is appended; backup first; existing content untouched)", "block    $($block -replace "`n", ' / ')") {
                    Update-CargoConfig -File $cfg -Id 'target-dir' -Block $block
                } -Vars @{ cfg = $cfg; block = $block }))
    }
    return $steps
}

function Get-PrefsValues {
    param([string[]]$Selected, [string]$Sdk, [string]$ZigExe, [string]$CargoExe, [switch]$Actual)
    $v = [ordered]@{}
    if ($Actual) {
        $cargo = Find-Tool 'cargo'; if ($cargo) { $v['tool_cargo'] = $cargo }
        if ($Selected -contains 'android') {
            if (Test-Path $Sdk) { $v['tool_android_sdk'] = $Sdk }
            $ndk = Get-ChildItem (Join-Path $Sdk 'ndk') -Directory -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1; if ($ndk) { $v['tool_android_ndk'] = $ndk.FullName }
            $j = Get-JdkHome; if ($j) { $v['tool_java'] = $j }
        }
        if ($Selected -contains 'linux-cross' -and (Test-Path $ZigExe)) { $v['tool_zig'] = $ZigExe }
        if ($Selected -contains 'cpp') { $cm = Find-Tool 'cmake'; if ($cm) { $v['tool_cmake'] = $cm } }
        return $v
    }
    $v['tool_cargo'] = $CargoExe
    if ($Selected -contains 'android') { $v['tool_android_sdk'] = $Sdk; $v['tool_android_ndk'] = (Join-Path $Sdk "ndk\$($script:Data['android']['ndk'])"); $v['tool_java'] = '<JDK folder>' }
    if ($Selected -contains 'linux-cross') { $v['tool_zig'] = $ZigExe }
    if ($Selected -contains 'cpp') { $v['tool_cmake'] = '<cmake.exe>' }
    return $v
}

function Backup-File {
    param([string]$File)
    if (-not (Test-Path -LiteralPath $File)) { return $null }
    $b = "$File.piska-backup-" + (Get-Date).ToString('yyyyMMdd-HHmmss')
    Copy-Item -LiteralPath $File -Destination $b
    return $b
}

function Update-PreferencesFile {
    param([string]$File, $Values)
    if ($Values.Count -eq 0) { Write-Info '  no tool paths to write'; return }
    $text = ''; if (Test-Path -LiteralPath $File) { $text = [IO.File]::ReadAllText($File) }
    $r = Merge-Preferences -Text $text -Values $Values
    $overwrite = @()
    foreach ($k in $r.Kept) {
        if (Confirm-Action "editor.ron already sets $k. Replace it with $($Values[$k])?" $false -Key 'prefs-overwrite' -Subject $k) { $overwrite += $k }
        else { Write-Info "  kept your $k" }
    }
    if ($overwrite.Count -gt 0) { $r = Merge-Preferences -Text $text -Values $Values -Overwrite $overwrite }
    if ($r.Set.Count -eq 0) { Write-Info '  preferences already up to date'; return }
    $backup = Backup-File $File
    New-Item -ItemType Directory -Force -Path (Split-Path $File) | Out-Null
    [IO.File]::WriteAllText($File, $r.Text, (New-Object System.Text.UTF8Encoding($false)))
    Add-ManifestEntry -Kind 'prefs' -Id 'editor.ron' -Detail $File -Previous $(if ($backup) { $backup } else { '' }) -Component 'config'
    Write-Info "  wrote $($r.Set -join ', ') to $File$(if ($backup) { " (backup: $backup)" })"
}

function Update-CargoConfig {
    param([string]$File, [string]$Id, [string]$Block)
    $text = ''; if (Test-Path -LiteralPath $File) { $text = [IO.File]::ReadAllText($File) }
    if ($text -match '(?m)^\s*target-dir\s*=') { Write-Warn "$File already sets target-dir: left untouched"; return }
    if (-not (Confirm-Action "Append a [build] target-dir block to $File?" $true -Key 'cargo-config' -Subject $File)) { Write-Info '  skipped'; return }
    $backup = Backup-File $File
    New-Item -ItemType Directory -Force -Path (Split-Path $File) | Out-Null
    [IO.File]::WriteAllText($File, (Add-DelimitedBlock -Text $text -Id $Id -Block $Block), (New-Object System.Text.UTF8Encoding($false)))
    Add-ManifestEntry -Kind 'cargo-config' -Id $Id -Detail $File -Previous $(if ($backup) { $backup } else { '' }) -Component 'config'
}

# ----------------------------------------------------------------------------------------
# Uninstall
# ----------------------------------------------------------------------------------------

function Invoke-UninstallAction {
    param($Item)
    $e = $Item.Entry
    switch ($Item.Action) {
        'delete-file' { Remove-Item -LiteralPath $Item.Target -Force -ErrorAction SilentlyContinue }
        'delete-dir' { if (Test-Path $Item.Target) { Remove-Item -LiteralPath $Item.Target -Recurse -Force } }
        'restore-env' { [Environment]::SetEnvironmentVariable($e.id, $(if ($e.previous) { $e.previous } else { $null }), 'User') }
        'remove-path' { }
        'restore-assoc' {
            Remove-Item -Path "HKCU:\Software\Classes\$($e.detail)" -Recurse -Force -ErrorAction SilentlyContinue
            if ($e.previous) { Set-ItemProperty -Path "HKCU:\Software\Classes\$($e.id)" -Name '(default)' -Value $e.previous } else { Remove-Item -Path "HKCU:\Software\Classes\$($e.id)" -Recurse -Force -ErrorAction SilentlyContinue }
        }
        'restore-prefs' { if ($e.previous -and (Test-Path $e.previous)) { Copy-Item -LiteralPath $e.previous -Destination $e.detail -Force } }
        'remove-cargo-block' {
            $t = [IO.File]::ReadAllText($e.detail); [IO.File]::WriteAllText($e.detail, (Remove-DelimitedBlock -Text $t -Id $e.id), (New-Object System.Text.UTF8Encoding($false)))
        }
        'cargo-uninstall' { [void](Invoke-Native -File (Find-Tool 'cargo') -Arguments @('uninstall', $e.id)) }
        'rustup-target-remove' { [void](Invoke-Native -File (Find-Tool 'rustup') -Arguments @('target', 'remove', $e.id)) }
        'rustup-self-uninstall' { [void](Invoke-Native -File (Find-Tool 'rustup') -Arguments @('self', 'uninstall', '-y')) }
        'winget-uninstall' { [void](Invoke-Native -File (Find-Tool 'winget') -Arguments @('uninstall', '--id', $e.id, '--exact', '--silent')) }
    }
}

function Start-Uninstall {
    $manifest = Read-JsonFile $script:Dirs.ManifestFile
    if ($null -eq $manifest -or @($manifest.entries).Count -eq 0) {
        if ($script:Json) { Write-Event 'uninstall-plan' ([ordered]@{ manifest = $script:Dirs.ManifestFile; dry_run = [bool]$script:Opt.DryRun; items = @() }) }
        Write-Info "Nothing to remove: no install manifest at $($script:Dirs.ManifestFile)."; return 0 }
    $plan = Get-UninstallPlan $manifest
    if ($script:Json) {
        $items = @($plan | ForEach-Object { [ordered]@{ action = $_.Action; target = $_.Target; ask = [bool]$_.Ask; kind = $_.Entry.kind; component = $_.Entry.component } })
        Write-Event 'uninstall-plan' ([ordered]@{ manifest = $script:Dirs.ManifestFile; dry_run = [bool]$script:Opt.DryRun; items = $items })
    }
    Write-Info "Uninstall plan (only what this installer added; programs that were already on the machine are not listed):"
    foreach ($p in $plan) { Write-Info ("  {0,-22} {1}{2}" -f $p.Action, $p.Target, $(if ($p.Ask) { '   [asks first]' } else { '' })) }
    if ($script:Opt.DryRun) { Write-Info 'dry run: nothing was changed.'; return 0 }
    if (-not (Confirm-Action 'Proceed?' $false -Key 'uninstall-proceed')) { return $script:ExitDeclined }
    foreach ($p in $plan) {
        if ($p.Ask -and -not (Confirm-Action "Remove $($p.Target) ($($p.Action))?" $false -Key 'uninstall-item' -Subject $p.Target)) { Write-Info "  kept $($p.Target)"; continue }
        try { Invoke-UninstallAction $p; Write-Info "  done: $($p.Action) $($p.Target)" } catch { Write-Warn "$($p.Action) $($p.Target): $($_.Exception.Message)" }
    }
    Remove-Item -LiteralPath $script:Dirs.ManifestFile -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $script:Dirs.StateFile -Force -ErrorAction SilentlyContinue
    return 0
}

# ----------------------------------------------------------------------------------------
# Main
# ----------------------------------------------------------------------------------------

function Initialize-Data {
    $here = $PSScriptRoot
    $candidate = if ($here) { Join-Path $here 'packages.toml' } else { '' }
    if ($candidate -and (Test-Path $candidate)) { $script:DataPath = $candidate; return ConvertFrom-PiskaToml ([IO.File]::ReadAllText($candidate)) }
    # Started from the network: fetch the data file next to the script, show its checksum, keep it for later runs.
    $base = if ($BaseUrl) { $BaseUrl } else { 'https://raw.githubusercontent.com/boobser1488/piska-engine/main/install' }
    $local = if ($env:LOCALAPPDATA) { $env:LOCALAPPDATA } else { [IO.Path]::GetTempPath() }
    $dir = Join-Path $local 'PiskaEngine\install'
    New-Item -ItemType Directory -Force -Path $dir | Out-Null
    $file = Join-Path $dir 'packages.toml'
    $note = "No local checkout: fetching $base/packages.toml"
    if ($script:Json) { Write-Event 'log' ([ordered]@{ level = 'info'; step = $null; text = $note }) } else { Write-Host $note }
    Invoke-Download -Url "$base/packages.toml" -OutFile $file
    $note = "packages.toml sha256: $(Get-FileSha256 $file)  (compare with the repository if you want to be sure)"
    if ($script:Json) { Write-Event 'log' ([ordered]@{ level = 'info'; step = $null; text = $note }) } else { Write-Host $note }
    $script:DataPath = $file
    return ConvertFrom-PiskaToml ([IO.File]::ReadAllText($file))
}

function Get-PlanEvent {
    # The "plan" event of one step: the same facts as the human plan lines, plus structured fields.
    param($Step, [int]$N, [int]$Total, [bool]$Skip, [string]$Reason, $Found)
    $foundText = if ($Found) { [string]$Found } else { $null }
    $details = Get-FoundDetails $foundText
    $downloads = @($Step.Downloads | Where-Object { $_ } | ForEach-Object { Get-DownloadInfo $_ })
    $size = 0; foreach ($dl in $downloads) { if ($dl['size_mb']) { $size += [int]$dl['size_mb'] } }
    $lines = @($Step.Plan | ForEach-Object {
            $parts = ([string]$_) -split '\s+', 2
            [ordered]@{ kind = $parts[0]; text = $(if ($parts.Count -gt 1) { $parts[1] } else { '' }) }
        })
    return [ordered]@{
        n = $N; total = $Total; id = $Step.Id; component = $Step.Component; title = $Step.Title
        skip = $Skip; reason = $(if ($Skip) { $Reason } else { $null }); found = $foundText; version = $details.version; path = $details.path
        elevation = $(if ($Step.Elevation) { $Step.Elevation } else { $null })
        licence = $(if ($Step.Licence) { [ordered]@{ id = $Step.LicenceId; name = $Step.Licence; url = $Step.LicenceUrl } } else { $null })
        skippable = ($script:SkippableSteps -contains $Step.Id)
        download_mb = $size; downloads = $downloads; lines = $lines
    }
}

function Test-SkipRequested {
    # -SkipSteps: $true for a listed step that may be skipped; a listed step that may not is reported and runs.
    param([string]$Id)
    if (@($script:Opt['SkipSteps']) -notcontains $Id) { return $false }
    if ($script:SkippableSteps -contains $Id) { return $true }
    Write-Warn "step $Id cannot be skipped (later steps need it): it runs as usual"
    return $false
}

function Show-Plan {
    param($Steps, [string]$Mode)
    $n = 0
    foreach ($s in $Steps) {
        $n++
        $script:CurrentStep = $s.Id
        $found = Invoke-Block $s.Detect $s.Vars
        $detected = [bool]$found
        if ($script:Opt.AssumeClean) { $detected = $false }
        $requested = Test-SkipRequested $s.Id
        $skip = $requested -or (Get-ResumeSkip -State $script:State -Id $s.Id -Detected $detected -Mode $Mode)
        $reason = if ($requested) { 'requested' } elseif ($detected) { 'found' } else { 'done-earlier' }
        if ($script:Json) { Write-Event 'plan' (Get-PlanEvent -Step $s -N $n -Total $Steps.Count -Skip $skip -Reason $reason -Found $found); continue }
        if ($skip) { Write-Info ("[{0}/{1}] SKIP {2} - {3}{4}" -f $n, $Steps.Count, $s.Id, $s.Title, $(if ($requested) { ' (skipped on request)' } elseif ($found) { " (found: $found)" } else { ' (done earlier)' })); continue }
        Write-Info ("[{0}/{1}] STEP {2} - {3}" -f $n, $Steps.Count, $s.Id, $s.Title)
        if ($s.Elevation) { Write-Info "         elevation: $($s.Elevation)" }
        if ($s.Licence) { Write-Info "         licence: $($s.Licence) ($($s.LicenceUrl)) - accepted only with -AcceptLicenses or your answer" }
        foreach ($l in $s.Plan) { Write-Info "         $l" }
    }
    $script:CurrentStep = $null
}

function Write-Hello {
    # -Json: the first event of a run: what, where, and how much space is free there.
    param([string[]]$Selected, [string]$Mode)
    Write-Event 'hello' ([ordered]@{
            protocol = 1; script = 'install.ps1'; shell = "PowerShell $($PSVersionTable.PSVersion) $($PSVersionTable.PSEdition)"; os = 'windows'
            mode = $Mode; dry_run = [bool]$script:Opt.DryRun; assume_clean = [bool]$script:Opt.AssumeClean; components = @($Selected)
            install_dir = $script:Dirs.Install; repo_dir = $script:RepoDir; in_checkout = [bool]$script:InCheckout; cache = $script:Dirs.Cache
            state_file = $script:Dirs.StateFile; manifest_file = $script:Dirs.ManifestFile; prefs_file = $script:Dirs.Prefs
            log = $script:LogFile; free_bytes = (Get-FreeBytes $script:Dirs.Install); is_admin = (Test-IsAdmin)
        })
}

function Write-Summary {
    # -Json: the last event, always written (also after an error); "exit" is the process exit code.
    param([int]$Code)
    $dirs = $script:Dirs
    $userEnv = [ordered]@{}
    foreach ($name in 'ANDROID_HOME', 'ANDROID_NDK_HOME', 'JAVA_HOME', 'PISKA_ZIG') { $v = [Environment]::GetEnvironmentVariable($name, 'User'); if ($v) { $userEnv[$name] = $v } }
    $freshPath = (@([Environment]::GetEnvironmentVariable('Path', 'Machine'), [Environment]::GetEnvironmentVariable('Path', 'User')) | Where-Object { $_ }) -join ';'
    if ($dirs) { $freshPath = (@($freshPath, (Join-Path $dirs.CargoHome 'bin')) | Where-Object { $_ }) -join ';' }
    Write-Event 'summary' ([ordered]@{
            exit = $Code; ok = ($Code -eq 0); dry_run = [bool]$script:Opt['DryRun']; failed_step = $script:FailedStep; error = $script:FailedError
            reboot = [bool]$script:RebootNeeded; elevation_refused = [bool]$script:ElevationRefused
            steps = [ordered]@{ total = $script:Stats.total; done = $script:Stats.done; skipped = $script:Stats.skipped; failed = $script:Stats.failed }
            log = $script:LogFile; state_file = $(if ($dirs) { $dirs.StateFile } else { $null }); manifest_file = $(if ($dirs) { $dirs.ManifestFile } else { $null })
            repo_dir = $script:RepoDir; path = $freshPath; env = $userEnv
        })
}

function Start-Install {
    param([string[]]$Selected)
    $mode = if ($Repair) { 'repair' } elseif ($Resume) { 'resume' } else { 'normal' }
    $script:SelectedNow = $Selected
    $steps = Get-Steps -Selected $Selected
    $script:Stats.total = $steps.Count
    if (-not $script:Opt.DryRun) { $script:LogFile = Join-Path $script:Dirs.Logs ("install-{0}.log" -f (Get-Date).ToString('yyyyMMdd-HHmmss')) }
    if ($script:Json) { Write-Hello -Selected $Selected -Mode $mode }
    Write-Info ("Components: {0}" -f ($Selected -join ', '))
    if ($script:Opt.DryRun) {
        Write-Info 'DRY RUN: nothing is installed, downloaded or changed. Detection below only reads.'
        Show-Plan -Steps $steps -Mode $mode
        Write-Info 'DRY RUN finished: no changes were made.'
        return 0
    }
    New-Item -ItemType Directory -Force -Path $script:Dirs.Logs | Out-Null
    Write-Info "Log: $($script:LogFile)"
    $script:State.components = $Selected
    $total = $steps.Count; $n = 0
    foreach ($s in $steps) {
        $n++
        $script:CurrentStep = $s.Id
        $found = Invoke-Block $s.Detect $s.Vars
        if (Test-SkipRequested $s.Id) {
            Write-Info ("[{0}/{1}] skip {2} (skipped on request)" -f $n, $total, $s.Title)
            if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'skip'; step = $s.Id; n = $n; total = $total; title = $s.Title; reason = 'requested'; found = $null }) }
            $script:State = Set-StepState $script:State $s.Id 'skipped'; $script:Stats.skipped++; continue
        }
        if (Get-ResumeSkip -State $script:State -Id $s.Id -Detected ([bool]$found) -Mode $mode) {
            Write-Info ("[{0}/{1}] skip {2} ({3})" -f $n, $total, $s.Title, $(if ($found) { $found } else { 'done earlier' }))
            if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'skip'; step = $s.Id; n = $n; total = $total; title = $s.Title; reason = $(if ($found) { 'found' } else { 'done-earlier' }); found = $(if ($found) { [string]$found } else { $null }) }) }
            $script:State = Set-StepState $script:State $s.Id 'done'; $script:Stats.skipped++; continue
        }
        Write-Info ("[{0}/{1}] {2}" -f $n, $total, $s.Title)
        if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'start'; step = $s.Id; n = $n; total = $total; title = $s.Title; component = $s.Component }) }
        if ($s.Licence) {
            if (-not (Confirm-License $s.Licence $s.LicenceUrl $s.LicenceId)) {
                Write-Info '  licence not accepted: stopping.'; Write-JsonFile $script:Dirs.StateFile $script:State
                if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'declined'; step = $s.Id; title = $s.Title; reason = 'licence'; licence = $s.LicenceId }) }
                return $script:ExitDeclined
            }
            if ($script:Json) { Write-Event 'licence' ([ordered]@{ step = $s.Id; licence = $s.LicenceId; name = $s.Licence; url = $s.LicenceUrl; accepted = $true }) }
        }
        if ($s.Elevation) {
            Write-Info "  $($s.Elevation)"
            if ($script:Json) { Write-Event 'elevation' ([ordered]@{ step = $s.Id; title = $s.Title; reason = $s.Elevation; confirmed = [bool]$script:Opt['ConfirmElevation'] }) }
            if (-not ($script:Opt['ConfirmElevation'] -or (Confirm-Action '  Continue?' $true -Key 'elevation' -Subject $s.Id))) {
                Write-Info '  declined: stopping.'; Write-JsonFile $script:Dirs.StateFile $script:State
                if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'declined'; step = $s.Id; title = $s.Title; reason = 'elevation'; licence = $null }) }
                return $script:ExitDeclined
            }
        }
        try {
            Invoke-Block $s.Run $s.Vars
            $script:State = Set-StepState $script:State $s.Id 'done'
            Write-JsonFile $script:Dirs.StateFile $script:State
            $script:Stats.done++
            if ($script:Json) { Write-Event 'step' ([ordered]@{ phase = 'done'; step = $s.Id; n = $n; total = $total; title = $s.Title }) }
        } catch {
            $script:State = Set-StepState $script:State $s.Id 'failed' $_.Exception.Message
            Write-JsonFile $script:Dirs.StateFile $script:State
            $script:Stats.failed++; $script:FailedStep = $s.Id; $script:FailedError = $_.Exception.Message
            Add-LogLine "FAILED step $($s.Id): $($_.Exception)"
            if ($script:Json) {
                Write-Event 'step' ([ordered]@{ phase = 'failed'; step = $s.Id; n = $n; total = $total; title = $s.Title; error = $_.Exception.Message; log = $script:LogFile
                        skippable = ($script:SkippableSteps -contains $s.Id); elevation_refused = [bool]$script:ElevationRefused })
            } else { Write-Host "FAILED step $($s.Id): $($_.Exception.Message)" -ForegroundColor Red }
            Write-Info "Fix the problem and run again with -Resume (steps that are done are skipped). Log: $($script:LogFile)"
            return $script:ExitStepFailed
        }
        if ($script:RebootNeeded) {
            Write-Info "A reboot is required to finish '$($s.Title)'. Restart Windows, then run: install.ps1 -Resume"
            if ($script:Json) { Write-Event 'reboot' ([ordered]@{ step = $s.Id; title = $s.Title }) }
            return $script:ExitReboot
        }
    }
    $script:CurrentStep = $null
    Write-Info 'All steps finished.'
    Show-Doctor -Selected $Selected
    Write-Info 'Open a NEW terminal so that PATH and the environment variables above take effect.'
    return 0
}

function Show-Doctor {
    param([string[]]$Selected)
    $exe = Get-ExePath $script:RepoDir 'piska-export'
    if (-not (Test-Path $exe)) { Write-Info "Diagnostics: build the engine component, then run `piska-export doctor` (or `cargo run -p piska-export -- doctor`)."; return }
    $list = ($Selected | Where-Object { $_ -ne 'engine' }) -join ','
    $a = @('doctor'); if ($list) { $a += @('--components', $list) }
    [void](Invoke-Native -File $exe -Arguments $a)
}

function Show-Menu {
    $items = $AllComponents
    Write-Host ''
    Write-Host 'Piska Engine installer - choose components (dependencies are added automatically):'
    for ($i = 0; $i -lt $items.Count; $i++) {
        $c = $script:Data["component.$($items[$i])"]
        Write-Host ("  {0}) {1,-12} {2}  [{3}]" -f ($i + 1), $items[$i], $c['title'], $c['size_windows'])
    }
    $answer = Read-Host 'Numbers or names separated by commas, "a" for all, Enter for core + engine'
    return (Resolve-MenuChoice -Answer $answer -Items $items -Default @('core', 'engine'))
}

function Main {
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) { Write-Host 'This is the Windows installer. On Linux / macOS run install/install.sh.'; return $script:ExitBadArgs }
    $script:Data = Initialize-Data
    $script:Opt = @{ Yes = [bool]$Yes; DryRun = ([bool]$DryRun -or [bool]$AssumeClean); AssumeClean = [bool]$AssumeClean; AcceptLicenses = [bool]$AcceptLicenses; NoPrefs = [bool]$NoPrefs
                     NoShortcuts = [bool]$NoShortcuts; DesktopShortcut = [bool]$DesktopShortcut; FileAssociations = [bool]$FileAssociations; AndroidSdk = $AndroidSdk
                     AcceptLicense = @($AcceptLicense | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ }); ConfirmElevation = [bool]$ConfirmElevation
                     SkipSteps = @($SkipSteps | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
    $script:Dirs = Get-Dirs -InstallDirArg $InstallDir -CacheArg $Cache
    $script:RebootNeeded = $false; $script:LogFile = $null
    if ($ListComponents) {
        foreach ($c in $AllComponents) { $e = $script:Data["component.$c"]; Write-Host ("{0,-12} {1,-8} {2}" -f $c, $e['size_windows'], $e['title']) }
        return 0
    }
    if ($Uninstall) { return (Start-Uninstall) }
    # In a checkout (install/install.ps1 next to Cargo.toml) the sources are used as they are.
    $script:InCheckout = $false
    $script:RepoDir = Join-Path $script:Dirs.Install 'engine'
    if ($PSScriptRoot) {
        $root = Split-Path $PSScriptRoot -Parent
        if ((Test-Path (Join-Path $root 'Cargo.toml')) -and (Test-Path (Join-Path $root 'tools\piska-export'))) { $script:InCheckout = $true; $script:RepoDir = $root }
    }
    # -SourceDir: an existing checkout chosen by the user (the graphical installer runs this script from a temp folder).
    if ($SourceDir) {
        if (-not ((Test-Path (Join-Path $SourceDir 'Cargo.toml')) -and (Test-Path (Join-Path $SourceDir 'tools\piska-export')))) {
            $msg = "-SourceDir $SourceDir is not an engine checkout (no Cargo.toml and tools\piska-export there)"
            if ($script:Json) { Write-Event 'error' ([ordered]@{ text = $msg }) } else { Write-Host "error: $msg" -ForegroundColor Red }
            return $script:ExitBadArgs
        }
        $script:InCheckout = $true; $script:RepoDir = (Resolve-Path -LiteralPath $SourceDir).ProviderPath
    }
    $script:SelfPath = if ($PSCommandPath) { $PSCommandPath } else { Join-Path $script:Dirs.Install 'install.ps1' }
    $script:State = Read-JsonFile $script:Dirs.StateFile
    if ($null -eq $script:State) { $script:State = [pscustomobject]@{ version = 1; components = @(); steps = @() } }
    $requested = $Components
    if (-not $requested -and ($Resume -or $Repair) -and @($script:State.components).Count -gt 0) { $requested = @($script:State.components) }
    if (-not $requested) {
        if ($Yes -or $script:Opt.DryRun) { if ($Yes) { $requested = @('core', 'engine'); Write-Info 'No -Components given: using core,engine.' } else { $requested = @('all') } }
        elseif ($script:Json) { Write-Event 'error' ([ordered]@{ text = '-Json needs -Components (or -DryRun / -Yes / -Resume)' }); return $script:ExitBadArgs }
        else { $requested = Show-Menu }
    }
    try { $selected = Resolve-Components -Requested $requested -Data $script:Data } catch {
        if ($script:Json) { Write-Event 'error' ([ordered]@{ text = $_.Exception.Message }) } else { Write-Host "error: $($_.Exception.Message)" -ForegroundColor Red }
        return $script:ExitBadArgs
    }
    return (Start-Install -Selected $selected)
}

# Tests dot-source this file with PISKA_INSTALL_NOMAIN=1 to reach the functions above.
if (-not $env:PISKA_INSTALL_NOMAIN) {
    if ($script:Json) {
        # -Json: the stream always ends with a "summary" event, also after an unexpected error.
        try { $code = Main } catch { Write-Event 'error' ([ordered]@{ text = $_.Exception.Message }); $code = $script:ExitStepFailed }
        if ($null -eq $code) { $code = 0 }
        Write-Summary ([int]$code)
    } else { $code = Main }
    if ($null -eq $code) { $code = 0 }
    if ($MyInvocation.InvocationName -ne '&' -and $PSCommandPath) { exit $code } else { if ($code -ne 0) { throw "install failed with exit code $code" } }
}
