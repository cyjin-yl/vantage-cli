$ErrorActionPreference = 'Continue'
$bin = 'D:\YanduBuildCache\cargo-target\debug\vantage.exe'
$probe = 'D:\YanduBuildCache\cargo-target\debug\examples\charge_domain_probe.exe'
$outDir = 'C:\Users\ezra\evidence\acc-20261102'
New-Item -ItemType Directory -Force $outDir | Out-Null
$log = New-Object 'System.Collections.Generic.List[string]'

function Log([string]$s) { $log.Add($s); Write-Host $s }
function Run([string]$label, [string]$argv) {
    Log("--- $label : vantage $argv")
    $out = & $bin ($argv -split ' ') 2>&1 | Out-String
    Log($out.Trim())
    Log("exit=$LASTEXITCODE")
}
function RunProbe([string]$label) {
    Log("--- probe ($label)")
    $out = & $probe 2>&1 | Out-String
    Log($out.Trim())
}
function Hpd([string]$label) {
    Log("--- HPD registry ($label)")
    $key = 'HKLM:\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD'
    if (Test-Path $key) {
        $v = Get-ItemProperty -Path $key -ErrorAction SilentlyContinue
        foreach ($n in 'PresentLeave','Approach','Video','Privacy','Capability') {
            Log("  $n = $($v.$n)")
        }
    } else { Log('  HPD key missing') }
}
function Vendor([string]$label) {
    Log("--- vendor readback ($label)")
    $out = & powershell -ExecutionPolicy Bypass -File C:\Users\ezra\vendor-probe2.ps1 2>&1 | Out-String
    foreach ($line in ($out -split "`r?`n")) { if ($line -match 'GetDolbyEnabled|GetActiveProfile|GetCurrentMode|QueryVoiceIDStatus') { Log("  $($line.Trim())") } }
}

Log('########## BASELINE ##########')
RunProbe 'baseline'
Hpd 'baseline'
Vendor 'baseline'
Run 'charge-mode baseline' '--json battery charge-mode rapid'
Run 'night-charge baseline' '--json battery night-charge'
Run 'usb baseline' '--json --dry-run usb always-on off'

try {
    Log('########## charge-mode ##########')
    Run 'charge-mode dry-run' '--dry-run battery charge-mode normal'
    Run 'charge-mode write' 'battery --yes charge-mode normal'
    RunProbe 'after charge-mode write'
    Run 'charge-mode readback' '--json battery charge-mode'
} finally {
    Run 'charge-mode restore' 'battery --yes charge-mode rapid'
    RunProbe 'after charge-mode restore'
}

try {
    Log('########## night charge ##########')
    Run 'night-charge dry-run' '--dry-run battery night-charge on'
    Run 'night-charge write' 'battery --yes night-charge on'
    RunProbe 'after night-charge write'
} finally {
    Run 'night-charge restore' 'battery --yes night-charge off'
    RunProbe 'after night-charge restore'
}

try {
    Log('########## usb always-on ##########')
    Run 'usb always-on dry-run' '--dry-run usb always-on off'
    Run 'usb always-on write' 'usb --yes always-on off'
    RunProbe 'after usb always-on write'
} finally {
    Run 'usb always-on restore' 'usb --yes always-on on'
    RunProbe 'after usb always-on restore'
}

try {
    Log('########## usb charge-on-battery ##########')
    Run 'usb battery dry-run' '--dry-run usb charge-on-battery off'
    Run 'usb battery write' 'usb --yes charge-on-battery off'
    RunProbe 'after usb battery write'
} finally {
    Run 'usb battery restore' 'usb --yes charge-on-battery on'
    RunProbe 'after usb battery restore'
}

try {
    Log('########## dolby ##########')
    Run 'dolby dry-run' '--dry-run audio dolby movie'
    Run 'dolby write' 'audio --yes dolby movie'
    Vendor 'after dolby write'
} finally {
    Run 'dolby restore profile' 'audio --yes dolby dynamic'
    Run 'dolby restore off' 'audio --yes dolby off'
    Vendor 'after dolby restore'
}

try {
    Log('########## noise cancel ##########')
    Run 'noise dry-run' '--dry-run audio noise-cancel shared'
    Run 'noise write' 'audio --yes noise-cancel shared'
    Vendor 'after noise write'
} finally {
    Run 'noise restore' 'audio --yes noise-cancel off'
    Vendor 'after noise restore'
}

try {
    Log('########## smartsense pause-video ##########')
    Run 'sense dry-run' '--dry-run sense pause-video off'
    Run 'sense write' 'sense --yes pause-video off'
    Hpd 'after sense write'
} finally {
    Run 'sense restore' 'sense --yes pause-video on'
    Hpd 'after sense restore'
}

Log('########## firmware- and privilege-bound writes ##########')
Run 'fan auto' 'perf fan auto'
Run 'pl1' 'perf pl1 20'
Run 'pl2' 'perf pl2 28'

Log('########## FINAL STATE ##########')
RunProbe 'final'
Hpd 'final'
Vendor 'final'
Run 'final charge-mode' '--json battery charge-mode rapid'
Run 'final usb' '--json --dry-run usb always-on off'

Set-Content -Path (Join-Path $outDir 'acceptance-transcript.txt') -Value ($log -join "`r`n")
Write-Host "transcript written to $outDir"
