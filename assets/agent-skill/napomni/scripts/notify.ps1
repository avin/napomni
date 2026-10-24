param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Text)
$ErrorActionPreference = 'Stop'
$utf8 = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $utf8
$cli = (Get-Content -LiteralPath (Join-Path $PSScriptRoot '../cli-path.json') -Raw -Encoding UTF8 | ConvertFrom-Json).cli
$start = [System.Diagnostics.ProcessStartInfo]::new()
$start.FileName = $cli
$start.Arguments = 'notify --stdin'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardInput = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$start.StandardOutputEncoding = $utf8
$start.StandardErrorEncoding = $utf8
$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $start
try {
    [void]$process.Start()
    $bytes = $utf8.GetBytes($Text)
    $process.StandardInput.BaseStream.Write($bytes, 0, $bytes.Length)
    $process.StandardInput.BaseStream.Close()
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    [Console]::Out.Write($stdout)
    [Console]::Error.Write($stderr)
    $result = $process.ExitCode
} finally {
    $process.Dispose()
}
exit $result
