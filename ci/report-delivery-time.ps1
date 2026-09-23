# #1217: JSON entrypoint for the offline delivery-time reader.
[CmdletBinding()]
param(
    [string] $Directory,
    [Nullable[int]] $PullRequest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ([string]::IsNullOrWhiteSpace($Directory)) { $Directory = Join-Path $PSScriptRoot '../.factory/gate-runs' }
. (Join-Path $PSScriptRoot 'delivery-time.ps1')

$read = Read-DeliveryReceipts -Directory $Directory -PullRequest $PullRequest
$report = Measure-DeliveryTime -ReadResult $read
$report | ConvertTo-Json -Depth 12 -Compress
if (@($read.indeterminate).Count -gt 0) { exit 4 }
exit 0
