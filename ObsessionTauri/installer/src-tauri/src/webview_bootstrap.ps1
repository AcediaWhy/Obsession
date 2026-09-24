$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Net.Http
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$bootstrapDirectory = Join-Path ([IO.Path]::GetTempPath()) ('Obsession-WebView2-' + [Guid]::NewGuid().ToString('N'))
$bootstrapPath = Join-Path $bootstrapDirectory 'MicrosoftEdgeWebview2Setup.exe'
$imageLock = $null
$client = $null
$response = $null
$inputStream = $null
$outputStream = $null
try {
    [void][IO.Directory]::CreateDirectory($bootstrapDirectory)
    $client = New-Object Net.Http.HttpClient
    $client.Timeout = [TimeSpan]::FromSeconds(90)
    $response = $client.GetAsync('https://go.microsoft.com/fwlink/p/?LinkId=2124703', [Net.Http.HttpCompletionOption]::ResponseHeadersRead).GetAwaiter().GetResult()
    [void]$response.EnsureSuccessStatusCode()
    if ($response.RequestMessage.RequestUri.Scheme -ne 'https') { throw 'Insecure bootstrap redirect' }
    $inputStream = $response.Content.ReadAsStreamAsync().GetAwaiter().GetResult()
    $outputStream = [IO.File]::Open($bootstrapPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $buffer = New-Object byte[] 65536
    $total = 0
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while ($true) {
        $readTask = $inputStream.ReadAsync($buffer, 0, $buffer.Length)
        if (-not $readTask.Wait(15000)) { throw 'Bootstrap download timed out' }
        $count = $readTask.Result
        if ($count -eq 0) { break }
        $total += $count
        if ($total -gt 16777216 -or [DateTime]::UtcNow -gt $deadline) { throw 'Bootstrap download exceeded bounds' }
        $outputStream.Write($buffer, 0, $count)
    }
    $outputStream.Dispose(); $outputStream = $null
    # Lock the exact bytes before signature verification, keeping writes/deletes
    # denied until the signed image has finished. No elevation of this script.
    $imageLock = [IO.File]::Open($bootstrapPath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $signature = Get-AuthenticodeSignature -LiteralPath $bootstrapPath
    if ($signature.Status -ne 'Valid' -or $null -eq $signature.SignerCertificate -or
        $signature.SignerCertificate.GetNameInfo([Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -ne 'Microsoft Corporation') {
        throw 'Bootstrap does not have a valid Microsoft signature'
    }
    $process = Start-Process -FilePath $bootstrapPath -ArgumentList '/silent', '/install' -WindowStyle Hidden -PassThru
    if (-not $process.WaitForExit(600000)) { throw 'WebView2 installation is still running; wait before retrying' }
    if ($process.ExitCode -ne 0 -and $process.ExitCode -ne 3010) { throw ('WebView2 setup exit code: ' + $process.ExitCode) }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    if ($outputStream) { $outputStream.Dispose() }
    if ($inputStream) { $inputStream.Dispose() }
    if ($response) { $response.Dispose() }
    if ($client) { $client.Dispose() }
    if ($imageLock) { $imageLock.Dispose() }
    # Exact files only: never recursively clean a caller-controlled temp tree.
    try { [IO.File]::Delete($bootstrapPath) } catch {}
    try { [IO.Directory]::Delete($bootstrapDirectory, $false) } catch {}
}
