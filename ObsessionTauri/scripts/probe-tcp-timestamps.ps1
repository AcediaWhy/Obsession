# One-shot diagnostic, explicitly approved by the user. No service/config edits.
# Must be launched elevated; always restore the captured timestamp setting.
$ErrorActionPreference = 'Stop'
$logPath = Join-Path $PSScriptRoot ("../artifacts/tcp-timestamps-probe-{0}.log" -f [guid]::NewGuid().ToString('N'))
Start-Transcript -LiteralPath $logPath -NoClobber | Out-Null
$original = $null
$attempted = $false
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Administrator approval is required'
    }
    $settings = Get-NetTCPSetting -SettingName Internet
    $original = $settings.Timestamps.ToString().ToLowerInvariant()
    if ($original -notin @('allowed', 'disabled', 'enabled')) { throw 'Unknown original timestamp setting' }
    Write-Host "ORIGINAL timestamps=$original"
    & netsh.exe interface tcp show global
    $urls = @(
        'https://discord.com/api/v10/gateway',
        'https://updates.discord.com/distributions/app/manifests/latest?channel=stable&platform=win&arch=x64'
    )
    foreach ($url in $urls) {
        & curl.exe --noproxy '*' -sS -o NUL --connect-timeout 4 --max-time 8 -w "BEFORE HTTP=%{http_code} TLS=%{time_appconnect} total=%{time_total}\n" $url
        Write-Host "curlExit=$LASTEXITCODE target=$($url.Split('/')[2])"
    }
    $attempted = $true
    & netsh.exe interface tcp set global timestamps=enabled
    if ($LASTEXITCODE -ne 0) { throw 'Could not enable timestamps' }
    $enabled = (Get-NetTCPSetting -SettingName Internet).Timestamps.ToString()
    Write-Host "TEST timestamps=$enabled"
    if ($enabled -ne 'Enabled') { throw 'Timestamp change did not take effect' }
    foreach ($tls in @('1.2', '1.3')) {
        foreach ($url in $urls) {
            & curl.exe --noproxy '*' -sS -o NUL "--tlsv$tls" --tls-max $tls --connect-timeout 4 --max-time 8 -w "ENABLED TLS$tls HTTP=%{http_code} TLS=%{time_appconnect} total=%{time_total}\n" $url
            Write-Host "curlExit=$LASTEXITCODE target=$($url.Split('/')[2])"
        }
    }
} finally {
    if ($attempted -and $null -ne $original) {
        & netsh.exe interface tcp set global "timestamps=$original"
        if ($LASTEXITCODE -ne 0) { Write-Host 'RESTORE FAILED: netsh returned an error' }
        $restored = (Get-NetTCPSetting -SettingName Internet).Timestamps.ToString().ToLowerInvariant()
        Write-Host "RESTORED timestamps=$restored expected=$original"
        if ($restored -ne $original) { Write-Host 'RESTORE FAILED: settings mismatch' }
    }
    Stop-Transcript | Out-Null
}
