$ErrorActionPreference = 'Stop'

$credential = $env:GRAPHHELM_EVIDENCE_TEST_CREDENTIAL
if (-not $credential) {
    throw 'GRAPHHELM_EVIDENCE_TEST_CREDENTIAL is required'
}
$secretFile = $env:GRAPHHELM_EVIDENCE_TEST_SECRET_FILE
if (-not $secretFile) {
    throw 'GRAPHHELM_EVIDENCE_TEST_SECRET_FILE is required'
}

for ($index = 1; $index -le 100; $index++) {
    Write-Output ('noise-{0:D3}' -f $index)
}
Write-Output 'test deterministic_postgres_failure ... FAILED'
[Console]::Error.WriteLine('failure class: assertion failed')
Write-Output "database url: postgres://gate:$credential@127.0.0.1:5432/fixture"
Write-Output "initdb password source: $secretFile"
[Console]::Error.WriteLine("PGPASSWORD=$credential")
exit 101
