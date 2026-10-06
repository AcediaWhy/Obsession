# Runs only the explicitly selected fixed-target local TLS record adapter.
# Does not set a system proxy, change DNS, start DPI capture or install a service.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-zA-Z0-9](?:[a-zA-Z0-9.-]{0,251}[a-zA-Z0-9])?$')]
    [string]$ServerName,

    [Parameter(Mandatory = $true)]
    [System.Net.IPAddress]$TargetAddress,

    [ValidateRange(1, 65535)]
    [int]$TargetPort = 443,

    [ValidateRange(1, 65535)]
    [int]$ListenPort = 18443,

    [ValidateRange(32, 16384)]
    [int]$FragmentBytes = 512,

    [string]$Executable
)

$ErrorActionPreference = 'Stop'
$taskProjectRoot = Split-Path -Parent $PSScriptRoot
if (-not $Executable) {
    $taskCandidates = @(
        (Join-Path $PSScriptRoot 'obsession-tls-record-proxy.exe'),
        (Join-Path $taskProjectRoot 'dist-release\experimental\obsession-tls-record-proxy.exe'),
        (Join-Path $taskProjectRoot 'tls-record-proxy\target\release\obsession-tls-record-proxy.exe')
    )
    $Executable = $taskCandidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}
if (-not $Executable -or -not (Test-Path -LiteralPath $Executable -PathType Leaf)) {
    throw 'Build the adapter first: cargo build --manifest-path tls-record-proxy/Cargo.toml --release --locked --offline'
}
$taskExecutable = (Resolve-Path -LiteralPath $Executable).Path
$taskTarget = if ($TargetAddress.AddressFamily -eq [System.Net.Sockets.AddressFamily]::InterNetworkV6) {
    '[' + $TargetAddress.ToString() + ']:' + $TargetPort
} else {
    $TargetAddress.ToString() + ':' + $TargetPort
}
$taskListen = '127.0.0.1:' + $ListenPort
Write-Host ('TLS records: ' + $taskListen + ' -> ' + $taskTarget + ' (SNI ' + $ServerName + ')')
Write-Host ('Example in a second terminal: curl.exe --noproxy "*" --connect-to "' + $ServerName + ':' + $TargetPort + ':127.0.0.1:' + $ListenPort + '" "https://' + $ServerName + ':' + $TargetPort + '/" --max-time 20')
Write-Host 'TLS certificates remain verified by the client. Press Ctrl+C to stop.'
& $taskExecutable '--listen' $taskListen '--target' $taskTarget '--server-name' $ServerName '--fragment-bytes' $FragmentBytes
if ($LASTEXITCODE -ne 0) { throw ('TLS record adapter exited with code ' + $LASTEXITCODE) }
