<#
.SYNOPSIS
  Self-tests of install/install.ps1 (no Pester). Nothing is installed, downloaded or changed:
  pure functions, parser checks, checksum checks on local fixtures, and dry-run golden files.

  pwsh -File install/tests/run.ps1            # run
  pwsh -File install/tests/run.ps1 -UpdateGolden   # rewrite install/tests/golden/windows-*.txt (review the diff!)
#>
param([switch]$UpdateGolden)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$instDir = Split-Path -Parent $here
$repo = Split-Path -Parent $instDir
$script = Join-Path $instDir 'install.ps1'
$goldenDir = Join-Path $here 'golden'
$script:failed = 0; $script:passed = 0

function Check {
    param([string]$Label, [scriptblock]$Body)
    try { & $Body; $script:passed++; Write-Host "ok   $Label" }
    catch { $script:failed++; Write-Host "FAIL $Label`n     $($_.Exception.Message)" -ForegroundColor Red }
}
function Assert { param([bool]$Cond, [string]$Msg) if (-not $Cond) { throw $Msg } }
function AssertEq { param($Got, $Want, [string]$Msg = '') if ("$Got" -ne "$Want") { throw "$Msg expected '$Want', got '$Got'" } }
function Throws { param([scriptblock]$Body, [string]$Pattern) try { & $Body } catch { if ($_.Exception.Message -match $Pattern) { return }; throw "threw '$($_.Exception.Message)', expected /$Pattern/" }; throw "did not throw (expected /$Pattern/)" }

# A scratch folder for fixtures (TEMP is under the caller's control).
$tmpRoot = Join-Path ([IO.Path]::GetTempPath()) ("piska-install-tests-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $tmpRoot | Out-Null

# ---- 1. parser checks -----------------------------------------------------------------
foreach ($f in @($script, (Join-Path $instDir 'tests\run.ps1'))) {
    Check "parse $(Split-Path $f -Leaf)" {
        $errors = $null; $tokens = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($f, [ref]$tokens, [ref]$errors)
        Assert (-not $errors) (($errors | ForEach-Object { "line $($_.Extent.StartLineNumber): $($_.Message)" }) -join '; ')
    }
}
Check 'scripts are plain ASCII (Windows PowerShell 5.1 reads BOM-less files as ANSI)' {
    foreach ($f in @($script, (Join-Path $instDir 'Install-PiskaEngine.cmd'), (Join-Path $instDir 'install.sh'))) {
        $bytes = [IO.File]::ReadAllBytes($f)
        Assert (-not ($bytes | Where-Object { $_ -gt 127 })) "$f contains non-ASCII bytes"
    }
}

# ---- load the functions ---------------------------------------------------------------
$env:PISKA_INSTALL_NOMAIN = '1'
. $script
$data = ConvertFrom-PiskaToml ([IO.File]::ReadAllText((Join-Path $instDir 'packages.toml')))
$script:Data = $data

# ---- 2. the data file ------------------------------------------------------------------
Check 'packages.toml parses and has the sections the installer needs' {
    foreach ($s in 'project', 'min', 'rust', 'winget', 'android', 'zig', 'component.core', 'linux.debian', 'macos') { Assert $data.Contains($s) "missing [$s]" }
    AssertEq $data['zig']['version'] '0.17.1' 'zig version'
    Assert ($data['android']['sdk_packages'].Count -eq 4) 'sdk_packages'
}
Check 'every download has an https URL and a pinned sha256 or a sha256_url' {
    $n = 0
    foreach ($k in $data.Keys | Where-Object { $_ -like 'download.*' }) { Assert-Download -Entry $data[$k] -Name $k; $n++ }
    Assert ($n -ge 10) "only $n downloads found"
}
Check 'a download entry without a checksum is refused' {
    Throws { Assert-Download -Entry @{ url = 'https://example.com/x.zip' } -Name 'x' } 'neither a pinned sha256'
    Throws { Assert-Download -Entry @{ url = 'http://example.com/x.zip'; sha256 = ('a' * 64) } -Name 'x' } 'https'
    Throws { Assert-Download -Entry @{ url = 'https://example.com/x.zip'; sha256 = 'abc' } -Name 'x' } 'neither a pinned sha256'
}
Check 'zig URLs carry the pinned version; the Android SDK packages match the version keys' {
    foreach ($k in $data.Keys | Where-Object { $_ -like 'download.zig_*' }) { Assert ($data[$k]['url'].Contains("/$($data['zig']['version'])/")) "$k url" }
    $a = $data['android']
    Assert ($a['sdk_packages'] -contains "platforms;$($a['platform'])") 'platform'
    Assert ($a['sdk_packages'] -contains "build-tools;$($a['build_tools'])") 'build-tools'
    Assert ($a['sdk_packages'] -contains "ndk;$($a['ndk'])") 'ndk'
}
Check 'versions in the data file are the ones piska-export suggests' {
    $android = [IO.File]::ReadAllText((Join-Path $repo 'tools\piska-export\src\android.rs'))
    Assert $android.Contains($data['android']['ndk']) "android.rs does not mention NDK $($data['android']['ndk'])"
    Assert $android.Contains($data['android']['build_tools']) "android.rs does not mention build-tools $($data['android']['build_tools'])"
}
Check 'URLs and versions live only in packages.toml (scripts do not hard-code them)' {
    foreach ($f in @($script, (Join-Path $instDir 'install.sh'))) {
        $text = [IO.File]::ReadAllText($f)
        foreach ($needle in 'commandlinetools-', 'ziglang.org/download/0', 'static.rust-lang.org/rustup', $data['android']['ndk'], $data['android']['build_tools'], "cargo-ndk $($data['android']['cargo_ndk'])", $data['zig']['version']) {
            Assert (-not $text.Contains($needle)) "$(Split-Path $f -Leaf) hard-codes '$needle'"
        }
    }
}
Check 'Linux package lists in packages.toml match docs/api/export.md section 10.1' {
    $doc = [IO.File]::ReadAllText((Join-Path $repo 'docs\api\export.md'))
    foreach ($p in $data['linux.debian']['build']) { Assert $doc.Contains($p) "export.md lacks debian package $p" }
    foreach ($p in $data['linux.fedora']['build']) { Assert $doc.Contains($p) "export.md lacks fedora package $p" }
    foreach ($p in $data['linux.arch']['build']) { Assert $doc.Contains($p) "export.md lacks arch package $p" }
}
Check 'docs/install.md states every version, size and package from packages.toml' {
    $doc = [IO.File]::ReadAllText((Join-Path $repo 'docs\install.md'))
    foreach ($v in $data['zig']['version'], $data['android']['ndk'], $data['android']['build_tools'], $data['android']['platform'], $data['android']['cargo_ndk'], $data['rust']['web_target'], $data['winget']['buildtools'], $data['winget']['jdk']) { Assert $doc.Contains($v) "install.md lacks $v" }
    foreach ($c in 'core', 'cpp', 'web', 'android', 'linux-cross', 'engine') {
        Assert $doc.Contains($data["component.$c"]['size_windows']) "install.md lacks the Windows size of $c ($($data["component.$c"]['size_windows']))"
        Assert $doc.Contains($data["component.$c"]['size_unix']) "install.md lacks the Linux size of $c"
    }
    foreach ($fam in 'debian', 'fedora', 'arch', 'suse') { foreach ($kind in 'build', 'run', 'cpp', 'jdk') { foreach ($p in $data["linux.$fam"][$kind]) { Assert $doc.Contains($p) "install.md lacks $fam package $p" } } }
}

# ---- 3. pure functions ---------------------------------------------------------------------
Check 'Resolve-Components adds dependencies and keeps the canonical order' {
    AssertEq ((Resolve-Components @('web') $data) -join ',') 'core,web'
    AssertEq ((Resolve-Components @('engine,android') $data) -join ',') 'core,android,engine'
    AssertEq ((Resolve-Components @('all') $data) -join ',') 'core,cpp,web,android,linux-cross,engine'
    AssertEq ((Resolve-Components @('core', 'core') $data) -join ',') 'core'
    Throws { Resolve-Components @('nope') $data } 'unknown component'
}
Check 'Resolve-MenuChoice' {
    $items = @('core', 'cpp', 'web')
    AssertEq (Resolve-MenuChoice '' $items @('core')) 'core'
    AssertEq ((Resolve-MenuChoice '1,3' $items @()) -join ',') 'core,web'
    AssertEq ((Resolve-MenuChoice 'a' $items @()) -join ',') 'core,cpp,web'
    AssertEq ((Resolve-MenuChoice 'cpp web' $items @()) -join ',') 'cpp,web'
    Throws { Resolve-MenuChoice '9' $items @() } 'not a menu item'
}
Check 'versions are read and compared' {
    AssertEq (Get-VersionFromText 'rustc 1.97.1 (8bab26f4f 2026-07-14)') '1.97.1'
    AssertEq (Get-VersionFromText 'cmake version 3.31.0-rc2') '3.31.0'
    Assert (Test-VersionAtLeast (Get-VersionFromText 'git version 2.47.0.windows.1') '2.30') 'git'
    Assert (-not (Test-VersionAtLeast (Get-VersionFromText 'rustc 1.70.0') '1.86')) 'old rust'
    Assert (-not (Test-VersionAtLeast $null '1.0')) 'null'
}
Check 'checksum verification: good file, tampered file, wrong and malformed hashes' {
    $file = Join-Path $tmpRoot 'fixture.bin'
    [IO.File]::WriteAllBytes($file, [Text.Encoding]::ASCII.GetBytes('piska fixture'))
    $good = Get-FileSha256 $file
    Assert (Test-FileChecksum $file $good) 'good'
    Assert (Test-FileChecksum $file $good.ToUpper()) 'upper case'
    [IO.File]::AppendAllText($file, 'x')
    Assert (-not (Test-FileChecksum $file $good)) 'tampered file must fail'
    Assert (-not (Test-FileChecksum $file 'abc')) 'short hash'
    Assert (-not (Test-FileChecksum (Join-Path $tmpRoot 'missing.bin') $good)) 'missing file'
    AssertEq (Get-FileSha256 (Join-Path $instDir 'packages.toml')).Length 64
}
Check 'a cached file that passes its checksum is reused without any network access' {
    $script:Dirs = @{ Cache = $tmpRoot }; $script:LogFile = $null; $script:Opt = @{ DryRun = $false }
    $file = Join-Path $tmpRoot 'tool.zip'
    [IO.File]::WriteAllBytes($file, [Text.Encoding]::ASCII.GetBytes('cached tool'))
    $entry = @{ url = 'https://example.invalid/dl/tool.zip'; sha256 = (Get-FileSha256 $file) }
    $got = Get-VerifiedFile 'tool' $entry 6>$null
    AssertEq $got $file
}
Check 'checksum text and zig index.json parsing' {
    $h = 'a' * 64
    AssertEq (Get-Sha256FromText "$h  rustup-init.exe") $h
    AssertEq (Get-Sha256FromText "$h") $h
    Assert ($null -eq (Get-Sha256FromText 'no hash here')) 'none'
    $json = '{"0.17.1":{"date":"2026-11-02","x86_64-windows":{"tarball":"https://ziglang.org/download/0.17.1/zig-x86_64-windows-0.17.1.zip","shasum":"' + ('b' * 64) + '","size":"1"}},"master":{}}'
    AssertEq (Get-ZigIndexSha $json '0.17.1' 'x86_64-windows') ('b' * 64)
    Assert ($null -eq (Get-ZigIndexSha $json '9.9.9' 'x86_64-windows')) 'unknown version'
    Assert ($null -eq (Get-ZigIndexSha $json '0.17.1' 'riscv-linux')) 'unknown platform'
}
Check 'delimited blocks: added once, existing text untouched, removable' {
    $orig = "[net]`ngit-fetch-with-cli = true`n"
    $new = Add-DelimitedBlock -Text $orig -Id 'target-dir' -Block "[build]`ntarget-dir = `"D:/t`""
    Assert $new.StartsWith($orig) 'existing content must stay first'
    Assert $new.Contains('target-dir = "D:/t"') 'block'
    AssertEq (Add-DelimitedBlock -Text $new -Id 'target-dir' -Block 'x') $new 'idempotent'
    AssertEq (Remove-DelimitedBlock -Text $new -Id 'target-dir').TrimEnd() $orig.TrimEnd()
}
Check 'preferences merge: adds missing keys, fills empty ones, keeps user values' {
    $text = "(`n    ui_scale: 1.25,`n    tool_cargo: C:\mine\cargo.exe,`n    tool_zig: ,`n    undo_limit: 100,`n)`n"
    $values = [ordered]@{ tool_cargo = 'C:\other\cargo.exe'; tool_zig = 'D:\zig\zig.exe'; tool_java = 'C:\jdk,17' }
    $r = Merge-Preferences -Text $text -Values $values
    AssertEq ($r.Kept -join ',') 'tool_cargo'
    AssertEq (($r.Set | Sort-Object) -join ',') 'tool_java,tool_zig'
    Assert $r.Text.Contains('tool_cargo: C:\mine\cargo.exe,') 'user value kept'
    Assert $r.Text.Contains('tool_zig: D:\zig\zig.exe,') 'empty value filled'
    Assert $r.Text.Contains('tool_java: C:\jdk%2C17,') 'comma escaped'
    Assert $r.Text.Contains('ui_scale: 1.25,') 'other keys untouched'
    Assert $r.Text.TrimEnd().EndsWith(')') 'still closed'
    $r2 = Merge-Preferences -Text $text -Values $values -Overwrite @('tool_cargo')
    Assert $r2.Text.Contains('tool_cargo: C:\other\cargo.exe,') 'overwrite on request'
    $again = Merge-Preferences -Text $r.Text -Values $values
    AssertEq $again.Set.Count 0 'second merge changes nothing'
}
Check 'preferences merge on a missing file and on a broken one' {
    $r = Merge-Preferences -Text '' -Values ([ordered]@{ tool_cargo = 'X' })
    Assert ($r.Text -match '(?s)^\(\r?\n    tool_cargo: X,\r?\n\)\r?\n$') "new file: $($r.Text)"
    Throws { Merge-Preferences -Text 'garbage without paren' -Values ([ordered]@{ a = 'b' }) } "closing"
}
Check 'preferences merge on a temp file is byte-stable for untouched lines (CRLF kept)' {
    $f = Join-Path $tmpRoot 'editor.ron'
    [IO.File]::WriteAllText($f, "(`r`n    vsync: true,`r`n)`r`n")
    $r = Merge-Preferences -Text ([IO.File]::ReadAllText($f)) -Values ([ordered]@{ tool_cargo = 'C:\c\cargo.exe' })
    Assert $r.Text.Contains("vsync: true,`r`n") 'CRLF'
    Assert ($r.Text -notmatch "(?<!`r)`n") 'no bare LF introduced'
}
Check 'state and resume logic' {
    $state = [pscustomobject]@{ version = 1; components = @('core'); steps = @() }
    $state = Set-StepState $state 'core.git' 'done'
    $state = Set-StepState $state 'core.rustup' 'failed' 'boom'
    $state = Set-StepState $state 'core.git' 'done'
    AssertEq @($state.steps).Count 2 'no duplicate entries'
    AssertEq ((Get-PendingSteps $state @('core.git', 'core.rustup', 'web.target')) -join ',') 'core.rustup,web.target'
    Assert (Get-ResumeSkip $state 'core.git' $false 'resume') 'done step is skipped when resuming'
    Assert (-not (Get-ResumeSkip $state 'core.git' $false 'repair')) 'repair ignores the state file'
    Assert (-not (Get-ResumeSkip $state 'core.rustup' $false 'resume')) 'failed step runs again'
    Assert (Get-ResumeSkip $state 'core.rustup' $true 'resume') 'a failed step that is detected is skipped'
    Assert (-not (Get-ResumeSkip $null 'x' $false 'normal')) 'no state'
}
Check 'uninstall plan comes from the manifest only, newest first, installed programs ask first' {
    $manifest = [pscustomobject]@{ entries = @(
            [pscustomobject]@{ kind = 'winget'; id = 'Kitware.CMake'; detail = ''; previous = ''; component = 'cpp' }
            [pscustomobject]@{ kind = 'rust-target'; id = 'wasm32-unknown-unknown'; detail = ''; previous = ''; component = 'web' }
            [pscustomobject]@{ kind = 'env'; id = 'ANDROID_HOME'; detail = 'C:\sdk'; previous = ''; component = 'android' }
            [pscustomobject]@{ kind = 'shortcut'; id = 'piska-hub'; detail = 'C:\sm\Piska Hub.lnk'; previous = ''; component = 'engine' }
            [pscustomobject]@{ kind = 'prefs'; id = 'editor.ron'; detail = 'C:\a\editor.ron'; previous = 'C:\a\editor.ron.bak'; component = 'config' }) }
    $plan = @(Get-UninstallPlan $manifest)
    AssertEq (($plan | ForEach-Object Action) -join ',') 'restore-prefs,delete-file,restore-env,rustup-target-remove,winget-uninstall'
    Assert ($plan[0].Ask -and $plan[4].Ask -and -not $plan[1].Ask) 'ask flags'
    AssertEq @(Get-UninstallPlan ([pscustomobject]@{ entries = @() })).Count 0 'empty manifest -> nothing'
}

# ---- 4. dry-run in-process: every combination of components ------------------------------------
$fake = Join-Path $tmpRoot 'fake-home'
Check 'dry-run of every one of the 63 component combinations plans correctly and touches nothing' {
    $all = @('core', 'cpp', 'web', 'android', 'linux-cross', 'engine')
    $script:Dirs = $null
    $env:LOCALAPPDATA = Join-Path $fake 'Local'; $env:APPDATA = Join-Path $fake 'Roaming'; $env:USERPROFILE = Join-Path $fake 'Home'; $env:CARGO_HOME = ''
    for ($mask = 1; $mask -lt 64; $mask++) {
        $pick = @(); for ($i = 0; $i -lt 6; $i++) { if ($mask -band (1 -shl $i)) { $pick += $all[$i] } }
        $script:Opt = @{ Yes = $true; DryRun = $true; AssumeClean = $true; AcceptLicenses = $false; NoPrefs = $false; NoShortcuts = $false; DesktopShortcut = $false; FileAssociations = $false; AndroidSdk = $null }
        $script:Dirs = Get-Dirs -InstallDirArg (Join-Path $fake 'Install') -CacheArg $null
        $script:InCheckout = $true; $script:RepoDir = $repo; $script:State = $null; $script:LogFile = $null
        $selected = Resolve-Components -Requested $pick -Data $data
        $script:SelectedNow = $selected
        $steps = Get-Steps -Selected $selected
        $out = (Show-Plan -Steps $steps -Mode 'normal' *>&1 | Out-String)
        foreach ($c in $selected) { Assert ($out -match "STEP $([regex]::Escape($c))\.") "mask $mask ($($pick -join ',')): no step for $c" }
        if ($pick -notcontains 'core') { Assert ($selected -contains 'core') "mask ${mask}: core not added" }
        Assert ($out -cnotmatch 'SKIP ') ("mask ${mask}: a clean machine must plan every step: " + (($out -split "`n" | Where-Object { $_ -cmatch 'SKIP ' }) -join ' | '))
        Assert ($out -notmatch 'Exception|cannot|is not recognized') "mask ${mask}: error text in the plan"
    }
    Assert (-not (Test-Path $fake)) 'the fake home was created: a dry run wrote to disk'
}

# ---- 5. dry-run through the real command line (golden files), in every PowerShell on this machine -------
$hosts = @()
$self = (Get-Process -Id $PID).Path; $hosts += $self
$other = if ($PSVersionTable.PSEdition -eq 'Core') { Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe' } else { (Get-Command pwsh -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Source) }
if ($other -and (Test-Path $other)) { $hosts += $other }

function Normalize { param([string]$Text, [string]$FakeRoot)
    $t = $Text -replace "`r", ''
    foreach ($p in @($repo, ($repo -replace '\\', '/'))) { $t = $t.Replace($p, '<REPO>') }
    $t = $t.Replace($FakeRoot, '<FAKE>')
    return $t.TrimEnd() + "`n"
}

$goldenSets = [ordered]@{
    'core' = 'core'; 'cpp' = 'cpp'; 'web' = 'web'; 'android' = 'android'; 'linux-cross' = 'linux-cross'; 'engine' = 'engine'; 'all' = 'all'
}
New-Item -ItemType Directory -Force -Path $goldenDir | Out-Null
foreach ($gname in $goldenSets.Keys) {
    foreach ($h in $hosts) {
        $label = "golden dry-run '$gname' in $(Split-Path $h -Leaf)"
        Check $label {
            $root = Join-Path $tmpRoot ("g-" + [Guid]::NewGuid().ToString('N').Substring(0, 6))
            $saved = @{}; foreach ($v in 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE', 'CARGO_HOME', 'PISKA_INSTALL_NOMAIN', 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_NDK_HOME', 'PISKA_ZIG') { $saved[$v] = [Environment]::GetEnvironmentVariable($v) }
            try {
                $env:LOCALAPPDATA = "$root\Local"; $env:APPDATA = "$root\Roaming"; $env:USERPROFILE = "$root\Home"; $env:CARGO_HOME = ''; $env:PISKA_INSTALL_NOMAIN = ''
                foreach ($v in 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_NDK_HOME', 'PISKA_ZIG') { Set-Item "Env:$v" '' }
                $raw = & $h -NoProfile -ExecutionPolicy Bypass -File $script -Components $goldenSets[$gname] -AssumeClean -InstallDir "$root\Install" 2>&1 | Out-String
                Assert ($LASTEXITCODE -eq 0) "exit code $LASTEXITCODE`n$raw"
            } finally { foreach ($v in $saved.Keys) { Set-Item "Env:$v" $saved[$v] } }
            # PowerShell itself caches startup data under LOCALAPPDATA\Microsoft; anything else would be ours.
            $made = @(Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike '*Microsoft*' })
            Assert ($made.Count -eq 0) ('dry run created files: ' + (($made | ForEach-Object FullName) -join ', '))
            $got = Normalize $raw $root
            $golden = Join-Path $goldenDir "windows-$gname.txt"
            if ($UpdateGolden -and $h -eq $self) { [IO.File]::WriteAllText($golden, $got, (New-Object Text.UTF8Encoding($false))) }
            Assert (Test-Path $golden) "no golden file $golden (run with -UpdateGolden)"
            $want = ([IO.File]::ReadAllText($golden) -replace "`r", '')
            if ($got -ne $want) {
                $d = Compare-Object ($want -split "`n") ($got -split "`n") | Select-Object -First 6 | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" }
                throw "output differs from $golden`n$($d -join "`n")"
            }
        }
    }
}
Check 'golden plans mention the safety-relevant facts (licence, UAC, checksum, no hidden downloads)' {
    $all = [IO.File]::ReadAllText((Join-Path $goldenDir 'windows-all.txt'))
    foreach ($needle in 'licence:', 'elevation:', 'verify   sha256', 'cross-checked with', 'DRY RUN finished: no changes were made.') { Assert $all.Contains($needle) "plan lacks '$needle'" }
    Assert ($all -notmatch '(?i)Invoke-Expression|iex ') 'plan runs no remote script'
}

# ---- 6. uninstall dry-run reads a manifest and changes nothing ---------------------------------------------------
Check 'uninstall dry-run lists the manifest actions and leaves the files in place' {
    $root = Join-Path $tmpRoot 'u'; $state = Join-Path $root 'Local\PiskaEngine\install'
    New-Item -ItemType Directory -Force -Path $state | Out-Null
    $victim = Join-Path $root 'Piska Hub.lnk'; Set-Content $victim 'x'
    $manifest = [pscustomobject]@{ version = 1; entries = @([pscustomobject]@{ kind = 'shortcut'; id = 'piska-hub'; detail = $victim; previous = ''; component = 'engine'; time = 'now' }) }
    ConvertTo-Json -InputObject $manifest -Depth 6 | Set-Content (Join-Path $state 'manifest.json')
    $saved = @{ L = $env:LOCALAPPDATA; N = $env:PISKA_INSTALL_NOMAIN }
    try { $env:LOCALAPPDATA = Join-Path $root 'Local'; $env:PISKA_INSTALL_NOMAIN = ''; $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Uninstall -DryRun 2>&1 | Out-String }
    finally { $env:LOCALAPPDATA = $saved.L; $env:PISKA_INSTALL_NOMAIN = $saved.N }
    Assert ($raw -match 'delete-file') $raw
    Assert (Test-Path $victim) 'dry run deleted a file'
    Assert (Test-Path (Join-Path $state 'manifest.json')) 'dry run removed the manifest'
}
Check 'bad component name exits with code 2 and a clear message' {
    $saved = $env:PISKA_INSTALL_NOMAIN; $env:PISKA_INSTALL_NOMAIN = ''
    try { $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Components nonsense -DryRun 2>&1 | Out-String; $code = $LASTEXITCODE } finally { $env:PISKA_INSTALL_NOMAIN = $saved }
    AssertEq $code 2; Assert ($raw -match 'unknown component') $raw
}
Check 'a licence is never accepted by -Yes alone' {
    $script:Opt = @{ Yes = $true; AcceptLicenses = $false; DryRun = $false }
    Throws { Confirm-License 'X' 'https://example.com' } 'AcceptLicenses'
    $script:Opt = @{ Yes = $true; AcceptLicenses = $true; DryRun = $false }
    Assert (Confirm-License 'X' 'https://example.com') 'accepted with the flag'
}

# ---- 7. -Json: the machine-readable stream of the graphical installer ------------------------------------------------
Check 'JSON writer: compact, pure ASCII, round-trips through ConvertFrom-Json' {
    $value = [ordered]@{ s = "a`"b\c`n" + [char]0x0416 + [char]0x00e9; n = 42; big = [long]8000000000; t = $true; f = $false; z = $null; list = @('x', 1); empty = @(); obj = [ordered]@{ k = 'v' } }
    $text = ConvertTo-JsonText $value
    Assert (-not ($text.ToCharArray() | Where-Object { [int]$_ -gt 126 -or [int]$_ -lt 32 })) "not ASCII: $text"
    Assert ($text.StartsWith('{"s":')) $text
    $back = $text | ConvertFrom-Json
    AssertEq $back.s $value.s 'string'
    AssertEq $back.n 42; AssertEq $back.big 8000000000; AssertEq $back.t $true; AssertEq $back.f $false
    Assert ($null -eq $back.z) 'null'
    AssertEq (@($back.list) -join ',') 'x,1'; AssertEq @($back.empty).Count 0; AssertEq $back.obj.k 'v'
}
Check 'answers to prompts: only an explicit yes counts, end of input is no' {
    foreach ($yes in 'yes', 'y', 'YES', 'true', 'accept', '{"answer":true}', ' yes ') { Assert (Test-JsonAnswer $yes) "'$yes' is a yes" }
    foreach ($no in 'no', 'n', '', 'maybe', '{"answer":false}') { Assert (-not (Test-JsonAnswer $no)) "'$no' is not a yes" }
    Assert (-not (Test-JsonAnswer $null)) 'end of input'
    $savedOut = [Console]::Out; $savedIn = [Console]::In
    try {
        $sw = New-Object IO.StringWriter; [Console]::SetOut($sw); [Console]::SetIn((New-Object IO.StringReader("yes`n")))
        $script:CurrentStep = 'core.buildtools'
        $got = Read-JsonAnswer -Kind 'licence' -Fields ([ordered]@{ licence = 'vs-buildtools'; name = 'X'; url = 'https://example.com' })
        $second = Read-JsonAnswer -Kind 'confirm' -Fields ([ordered]@{ key = 'k' })
    } finally { [Console]::SetOut($savedOut); [Console]::SetIn($savedIn); $script:CurrentStep = $null }
    Assert $got 'yes on stdin'
    Assert (-not $second) 'nothing more on stdin = no'
    $events = @($sw.ToString().Trim() -split "`r?`n" | ForEach-Object { $_ | ConvertFrom-Json })
    AssertEq $events[0].event 'prompt'; AssertEq $events[0].kind 'licence'; AssertEq $events[0].licence 'vs-buildtools'; AssertEq $events[0].step 'core.buildtools'
}
Check 'licences can be accepted one by one (-AcceptLicense), never by -Yes alone' {
    $script:Opt = @{ Yes = $true; AcceptLicenses = $false; AcceptLicense = @('vs-buildtools'); DryRun = $false }
    Assert (Confirm-License 'Visual Studio Build Tools' 'https://example.com' 'vs-buildtools') 'the named licence'
    Throws { Confirm-License 'the Android SDK' 'https://example.com' 'android-sdk' } 'AcceptLicenses'
}
Check 'download progress events for a local file (file:// URI, no network)' {
    $src = Join-Path $tmpRoot 'big.bin'; [IO.File]::WriteAllBytes($src, (New-Object byte[] 200000))
    $dst = Join-Path $tmpRoot 'big.copy'
    $savedOut = [Console]::Out
    try {
        $sw = New-Object IO.StringWriter; [Console]::SetOut($sw); $script:CurrentStep = 'linux-cross.zig'
        Invoke-DownloadWithProgress -Url ([Uri]$src).AbsoluteUri -OutFile $dst -EveryMs 0
    } finally { [Console]::SetOut($savedOut); $script:CurrentStep = $null }
    AssertEq (Get-Item $dst).Length 200000 'copied'
    $events = @($sw.ToString().Trim() -split "`r?`n" | ForEach-Object { $_ | ConvertFrom-Json })
    Assert ($events.Count -ge 2) "progress events: $($events.Count)"
    $last = $events[$events.Count - 1]
    AssertEq $last.event 'progress'; AssertEq $last.bytes 200000; AssertEq $last.total 200000; AssertEq $last.step 'linux-cross.zig'
    Assert ($events[0].bytes -le $last.bytes) 'bytes grow'
}

function Render-JsonPlan {
    # The human dry-run text rebuilt from -Json events: proves that both streams carry the same steps in the same order.
    param([object[]]$Events)
    $out = New-Object System.Collections.ArrayList
    foreach ($e in $Events) {
        switch ($e.event) {
            'log' { [void]$out.Add($e.text) }
            'plan' {
                if ($e.skip) {
                    $why = switch ($e.reason) { 'requested' { ' (skipped on request)' } 'found' { " (found: $($e.found))" } default { ' (done earlier)' } }
                    [void]$out.Add(("[{0}/{1}] SKIP {2} - {3}{4}" -f $e.n, $e.total, $e.id, $e.title, $why)); continue
                }
                [void]$out.Add(("[{0}/{1}] STEP {2} - {3}" -f $e.n, $e.total, $e.id, $e.title))
                if ($e.elevation) { [void]$out.Add("         elevation: $($e.elevation)") }
                if ($e.licence) { [void]$out.Add("         licence: $($e.licence.name) ($($e.licence.url)) - accepted only with -AcceptLicenses or your answer") }
                foreach ($l in @($e.lines)) { [void]$out.Add('         ' + $l.kind.PadRight(8) + ' ' + $l.text) }
            }
        }
    }
    return (($out -join "`n").TrimEnd() + "`n")
}
function ConvertFrom-JsonLines { param([string]$Raw) return @($Raw -split "`r?`n" | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json }) }

Check 'dry-run -Json of all 63 component combinations: the same steps in the same order as the human plan' {
    $all = @('core', 'cpp', 'web', 'android', 'linux-cross', 'engine')
    # Its own fake home: the child PowerShell of earlier checks caches data under the shared one.
    $fakeJ = Join-Path $tmpRoot 'fake-home-json'
    $env:LOCALAPPDATA = Join-Path $fakeJ 'Local'; $env:APPDATA = Join-Path $fakeJ 'Roaming'; $env:USERPROFILE = Join-Path $fakeJ 'Home'; $env:CARGO_HOME = ''
    $savedOut = [Console]::Out
    try {
        for ($mask = 1; $mask -lt 64; $mask++) {
            $pick = @(); for ($i = 0; $i -lt 6; $i++) { if ($mask -band (1 -shl $i)) { $pick += $all[$i] } }
            $script:Opt = @{ Yes = $true; DryRun = $true; AssumeClean = $true; AcceptLicenses = $false; NoPrefs = $false; NoShortcuts = $false; DesktopShortcut = $false; FileAssociations = $false; AndroidSdk = $null }
            $script:Dirs = Get-Dirs -InstallDirArg (Join-Path $fakeJ 'Install') -CacheArg $null
            $script:InCheckout = $true; $script:RepoDir = $repo; $script:State = $null; $script:LogFile = $null
            $selected = Resolve-Components -Requested $pick -Data $data
            $script:SelectedNow = $selected
            $steps = Get-Steps -Selected $selected
            $script:Json = $false
            $human = (@(Show-Plan -Steps $steps -Mode 'normal' 6>&1 | ForEach-Object { "$_" }) -join "`n")
            $sw = New-Object IO.StringWriter; [Console]::SetOut($sw); $script:Json = $true
            Show-Plan -Steps $steps -Mode 'normal'
            [Console]::SetOut($savedOut); $script:Json = $false
            $events = ConvertFrom-JsonLines $sw.ToString()
            $humanIds = @([regex]::Matches($human, '(?m)^\[\d+/\d+\] STEP (\S+)') | ForEach-Object { $_.Groups[1].Value })
            $jsonIds = @($events | Where-Object { $_.event -eq 'plan' } | ForEach-Object { $_.id })
            AssertEq ($jsonIds -join ',') ($humanIds -join ',') "mask $mask ($($pick -join ','))"
            AssertEq (Render-JsonPlan $events) ($human.TrimEnd() + "`n") "mask $mask rendered"
        }
    } finally { [Console]::SetOut($savedOut); $script:Json = $false }
    Assert (-not (Test-Path $fakeJ)) 'a -Json dry run wrote to disk'
}

function Normalize-Json {
    # Machine-specific values out of a -Json stream (paths, shell, free space, the PATH of this machine).
    param([string]$Text, [string]$FakeRoot)
    $t = $Text -replace "`r", ''
    foreach ($p in @($repo, ($repo -replace '\\', '/'), $FakeRoot)) {
        $t = $t.Replace(($p -replace '\\', '\\'), '<ROOT>').Replace($p, '<ROOT>')
    }
    $t = $t -replace '"shell":"[^"]*"', '"shell":"<SHELL>"' -replace '"free_bytes":(\d+|null)', '"free_bytes":"<N>"' -replace '"is_admin":(true|false)', '"is_admin":"<ADMIN>"'
    $t = $t -replace '"path":"[^"]*","env":\{[^}]*\}', '"path":"<PATH>","env":"<ENV>"'
    return $t.TrimEnd() + "`n"
}
foreach ($gname in $goldenSets.Keys) {
    foreach ($h in $hosts) {
        Check "golden -Json dry-run '$gname' in $(Split-Path $h -Leaf): matches the human golden and its own golden" {
            $root = Join-Path $tmpRoot ("j-" + [Guid]::NewGuid().ToString('N').Substring(0, 6))
            $saved = @{}; foreach ($v in 'LOCALAPPDATA', 'APPDATA', 'USERPROFILE', 'CARGO_HOME', 'PISKA_INSTALL_NOMAIN', 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_NDK_HOME', 'PISKA_ZIG') { $saved[$v] = [Environment]::GetEnvironmentVariable($v) }
            try {
                $env:LOCALAPPDATA = "$root\Local"; $env:APPDATA = "$root\Roaming"; $env:USERPROFILE = "$root\Home"; $env:CARGO_HOME = ''; $env:PISKA_INSTALL_NOMAIN = ''
                foreach ($v in 'JAVA_HOME', 'ANDROID_HOME', 'ANDROID_NDK_HOME', 'PISKA_ZIG') { Set-Item "Env:$v" '' }
                $raw = & $h -NoProfile -ExecutionPolicy Bypass -File $script -Components $goldenSets[$gname] -DryRun -AssumeClean -Json -InstallDir "$root\Install" 2>&1 | Out-String
                Assert ($LASTEXITCODE -eq 0) "exit code $LASTEXITCODE`n$raw"
            } finally { foreach ($v in $saved.Keys) { Set-Item "Env:$v" $saved[$v] } }
            $made = @(Get-ChildItem $root -Recurse -File -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notlike '*Microsoft*' })
            Assert ($made.Count -eq 0) ('dry run created files: ' + (($made | ForEach-Object FullName) -join ', '))
            $lines = @($raw -split "`r?`n" | Where-Object { $_.Trim() })
            foreach ($l in $lines) { Assert ($l.StartsWith('{"event":"')) "not an event line: $l" }
            $events = ConvertFrom-JsonLines $raw
            AssertEq $events[0].event 'hello'; AssertEq $events[$events.Count - 1].event 'summary'; AssertEq $events[$events.Count - 1].exit 0
            $human = [IO.File]::ReadAllText((Join-Path $goldenDir "windows-$gname.txt")) -replace "`r", ''
            AssertEq (Normalize (Render-JsonPlan $events) $root) $human 'rendered -Json plan vs the human golden'
            $got = Normalize-Json $raw $root
            $golden = Join-Path $goldenDir "windows-json-$gname.jsonl"
            if ($UpdateGolden -and $h -eq $self) { [IO.File]::WriteAllText($golden, $got, (New-Object Text.UTF8Encoding($false))) }
            Assert (Test-Path $golden) "no golden file $golden (run with -UpdateGolden)"
            $want = ([IO.File]::ReadAllText($golden) -replace "`r", '')
            if ($got -ne $want) {
                $d = Compare-Object ($want -split "`n") ($got -split "`n") | Select-Object -First 4 | ForEach-Object { "$($_.SideIndicator) $($_.InputObject.Substring(0, [Math]::Min(200, $_.InputObject.Length)))" }
                throw "output differs from $golden`n$($d -join "`n")"
            }
        }
    }
}
Check '-Json: bad arguments end with an error event, a summary and exit code 2' {
    $saved = $env:PISKA_INSTALL_NOMAIN; $env:PISKA_INSTALL_NOMAIN = ''
    try {
        $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Components nonsense -DryRun -Json 2>&1 | Out-String; $code = $LASTEXITCODE
        $raw2 = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Components core -DryRun -Json -SourceDir $tmpRoot 2>&1 | Out-String; $code2 = $LASTEXITCODE
    } finally { $env:PISKA_INSTALL_NOMAIN = $saved }
    AssertEq $code 2
    $events = ConvertFrom-JsonLines $raw
    AssertEq (($events | ForEach-Object event) -join ',') 'error,summary'
    Assert ($events[0].text -match 'unknown component') $raw
    AssertEq $events[1].exit 2
    AssertEq $code2 2; Assert ($raw2 -match 'not an engine checkout') $raw2
}
Check '-Json -SkipSteps: skippable steps are skipped on request, the others are refused with a warning' {
    $saved = $env:PISKA_INSTALL_NOMAIN; $env:PISKA_INSTALL_NOMAIN = ''
    try { $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Components core -DryRun -AssumeClean -Json -SkipSteps 'core.vulkan,core.git' -InstallDir (Join-Path $tmpRoot 'skip') 2>&1 | Out-String }
    finally { $env:PISKA_INSTALL_NOMAIN = $saved }
    $events = ConvertFrom-JsonLines $raw
    $vulkan = $events | Where-Object { $_.event -eq 'plan' -and $_.id -eq 'core.vulkan' }
    $git = $events | Where-Object { $_.event -eq 'plan' -and $_.id -eq 'core.git' }
    Assert ($vulkan.skip -and $vulkan.reason -eq 'requested') 'core.vulkan skipped on request'
    Assert (-not $git.skip) 'core.git cannot be skipped'
    Assert (@($events | Where-Object { $_.event -eq 'log' -and $_.level -eq 'warn' -and $_.text -match 'core.git cannot be skipped' }).Count -eq 1) 'warning for core.git'
}
Check '-Json -SourceDir: an existing checkout is used as the engine sources' {
    $saved = $env:PISKA_INSTALL_NOMAIN; $env:PISKA_INSTALL_NOMAIN = ''
    try { $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Components engine -DryRun -AssumeClean -Json -SourceDir $repo -InstallDir (Join-Path $tmpRoot 'src') 2>&1 | Out-String }
    finally { $env:PISKA_INSTALL_NOMAIN = $saved }
    $hello = (ConvertFrom-JsonLines $raw)[0]
    Assert $hello.in_checkout 'in_checkout'; AssertEq $hello.repo_dir $repo
}
Check 'uninstall -Json -DryRun: the manifest as an uninstall-plan event, nothing removed' {
    $root = Join-Path $tmpRoot 'uj'; $state = Join-Path $root 'Local\PiskaEngine\install'
    New-Item -ItemType Directory -Force -Path $state | Out-Null
    $victim = Join-Path $root 'Piska Hub.lnk'; Set-Content $victim 'x'
    $manifest = [pscustomobject]@{ version = 1; entries = @(
            [pscustomobject]@{ kind = 'winget'; id = 'Kitware.CMake'; detail = ''; previous = ''; component = 'cpp'; time = 'now' }
            [pscustomobject]@{ kind = 'shortcut'; id = 'piska-hub'; detail = $victim; previous = ''; component = 'engine'; time = 'now' }) }
    ConvertTo-Json -InputObject $manifest -Depth 6 | Set-Content (Join-Path $state 'manifest.json')
    $saved = @{ L = $env:LOCALAPPDATA; N = $env:PISKA_INSTALL_NOMAIN }
    try { $env:LOCALAPPDATA = Join-Path $root 'Local'; $env:PISKA_INSTALL_NOMAIN = ''; $raw = & $self -NoProfile -ExecutionPolicy Bypass -File $script -Uninstall -DryRun -Json 2>&1 | Out-String }
    finally { $env:LOCALAPPDATA = $saved.L; $env:PISKA_INSTALL_NOMAIN = $saved.N }
    $events = ConvertFrom-JsonLines $raw
    $plan = $events | Where-Object { $_.event -eq 'uninstall-plan' }
    AssertEq ((@($plan.items) | ForEach-Object action) -join ',') 'delete-file,winget-uninstall'
    Assert ((@($plan.items) | Where-Object { $_.action -eq 'winget-uninstall' }).ask) 'installed programs ask first'
    AssertEq $events[$events.Count - 1].event 'summary'
    Assert (Test-Path $victim) 'dry run deleted a file'
}

Remove-Item -LiteralPath $tmpRoot -Recurse -Force -ErrorAction SilentlyContinue
Write-Host ''
Write-Host ("{0} passed, {1} failed" -f $script:passed, $script:failed)
if ($script:failed -gt 0) { exit 1 }
