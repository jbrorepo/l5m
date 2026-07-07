# L5M live end-to-end test (PowerShell, Windows)
# Mirrors scripts/demo.sh and adds: durability-across-restart, auth-scope
# negative tests, and a hold-open window for live browser verification.
# Results -> reports\e2e_powershell_results.txt ; session -> reports\e2e_session.json

$ErrorActionPreference = 'Continue'
$repo    = Split-Path -Parent $PSScriptRoot
$exe     = Join-Path $repo 'target\release\l5m-server.exe'
$reports = Join-Path $repo 'reports'
New-Item -ItemType Directory -Force -Path $reports | Out-Null
$resultFile  = Join-Path $reports 'e2e_powershell_results.txt'
$sessionFile = Join-Path $reports 'e2e_session.json'
Remove-Item $resultFile, $sessionFile -ErrorAction SilentlyContinue

$port     = 18080
$base     = "http://127.0.0.1:$port"
$dataDir  = Join-Path $env:TEMP ("l5m-e2e-" + (Get-Random))
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
$writeKey = "e2e-write-$(Get-Random)"
$adminKey = "e2e-admin-$(Get-Random)"

$script:pass = 0; $script:fail = 0
function Log($msg) { $msg | Tee-Object -FilePath $resultFile -Append }
function Assert($name, $cond) {
  if ($cond) { $script:pass++; Log "PASS  $name" }
  else       { $script:fail++; Log "FAIL  $name" }
}

function Start-L5M {
  $env:L5M_BIND         = "127.0.0.1:$port"
  $env:L5M_DATA_DIR     = $dataDir
  $env:L5M_AUDIT_LOG    = Join-Path $dataDir 'audit.jsonl'
  $env:L5M_API_KEYS     = "${writeKey}:write,${adminKey}:admin"
  $env:L5M_RATE_PER_SEC = '50'
  $p = Start-Process -FilePath $exe -PassThru -WindowStyle Hidden
  foreach ($i in 1..50) {
    try { Invoke-WebRequest "$base/healthz" -UseBasicParsing -TimeoutSec 2 | Out-Null; return $p }
    catch { Start-Sleep -Milliseconds 200 }
  }
  return $p
}

function Req($tenant, $method, $path, $body, $key) {
  if (-not $key) { $key = $writeKey }
  $headers = @{ 'x-l5m-api-key' = $key; 'x-l5m-tenant' = "$tenant" }
  $args = @{ Uri = "$base$path"; Method = $method; Headers = $headers; UseBasicParsing = $true; TimeoutSec = 10 }
  if ($body) { $args.Body = $body; $args.ContentType = 'application/json' }
  try { (Invoke-WebRequest @args).Content } catch { "HTTP_ERROR $($_.Exception.Response.StatusCode.value__)" }
}

Log "L5M live E2E — $(Get-Date -Format o)"
Log "binary: $exe"
Log "data dir: $dataDir"
Log ""

# 1. Start server
Log "== 1/8 Start durable server (scoped keys, rate limit, audit log) =="
$server = Start-L5M
$health = try { (Invoke-WebRequest "$base/healthz" -UseBasicParsing -TimeoutSec 3).StatusCode } catch { 0 }
Assert "server up, /healthz returns 200" ($health -eq 200)

# 2. Durable writes
Log ""
Log "== 2/8 Tenant 7 writes 3 memories over HTTP =="
$m1 = Req 7 POST /v1/memories '{"capsule_id":"1","tenant_id":7,"claim":"the production db password is hunter2-kelpstone","evidence":"set during onboarding","source_id":1,"valid_from":1,"observed_at":1,"last_verified_at":1,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":9,"classification":1,"poison_risk":0}'
$m2 = Req 7 POST /v1/memories '{"capsule_id":"2","tenant_id":7,"claim":"the office was at 12 Amber Street","evidence":"lease v1","source_id":1,"valid_from":1000,"valid_until":5000,"observed_at":1000,"last_verified_at":1000,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}'
$m3 = Req 7 POST /v1/memories '{"capsule_id":"3","tenant_id":7,"claim":"the office is at 3 Cobalt Avenue","evidence":"lease v2","source_id":1,"valid_from":5000,"observed_at":5000,"last_verified_at":5000,"context_mask":"0xffff","policy_mask":"0xffff","trust_level":8,"classification":1,"poison_risk":0}'
Assert "3 writes acknowledged" (($m1 -notmatch 'HTTP_ERROR') -and ($m2 -notmatch 'HTTP_ERROR') -and ($m3 -notmatch 'HTTP_ERROR'))

# 3. Recall + tenant isolation
Log ""
Log "== 3/8 Recall for tenant 7; SAME query as tenant 42 must leak nothing =="
$t7 = Req 7 POST /v1/query '{"query":"production db password"}'
Assert "tenant 7 recalls its secret" ($t7 -match 'hunter2-kelpstone')
$t42 = Req 42 POST /v1/query '{"query":"the production db password is hunter2-kelpstone"}'
Assert "tenant 42 perfect-match query returns NO secret (gate before scoring)" ($t42 -notmatch 'hunter2')

# 4. Time-travel
Log ""
Log "== 4/8 Time-travel recall (as_of) =="
$then = Req 7 POST /v1/query '{"query":"where is the office","as_of":2000}'
$now  = Req 7 POST /v1/query '{"query":"where is the office","as_of":9000}'
Assert "as_of=2000 -> 12 Amber Street" ($then -match 'Amber Street')
Assert "as_of=2000 does NOT return current address" ($then -notmatch 'Cobalt Avenue')
Assert "as_of=9000 -> 3 Cobalt Avenue" ($now -match 'Cobalt Avenue')

# 5. Auth scopes
Log ""
Log "== 5/8 Auth-scope enforcement =="
$usageAdmin = Req 0 GET /v1/usage $null $adminKey
Assert "admin key reads /v1/usage" (($usageAdmin -notmatch 'HTTP_ERROR') -and ($usageAdmin -match '7'))
$usageWrite = Req 0 GET /v1/usage $null $writeKey
Assert "write key is DENIED on /v1/usage" ($usageWrite -match 'HTTP_ERROR (401|403)')
$noKey = try { (Invoke-WebRequest "$base/v1/query" -Method POST -Body '{"query":"x"}' -ContentType 'application/json' -Headers @{'x-l5m-tenant'='7'} -UseBasicParsing -TimeoutSec 5).StatusCode } catch { $_.Exception.Response.StatusCode.value__ }
Assert "request with no API key rejected (401/403)" ($noKey -in 401,403)

# 6. Audit chain
Log ""
Log "== 6/8 Tamper-evident audit chain =="
$audit = Req 7 GET /v1/audit/verify
Assert "audit chain verifies intact" ($audit -match '"intact":\s*true')
Log "audit response: $audit"

# 7. Durability across restart
Log ""
Log "== 7/8 Durability: kill server, restart on same data dir, recall =="
Stop-Process -Id $server.Id -Force; Start-Sleep -Seconds 1
$server = Start-L5M
$after = Req 7 POST /v1/query '{"query":"production db password"}'
Assert "acknowledged write survives restart (WAL durability)" ($after -match 'hunter2-kelpstone')
$t42b = Req 42 POST /v1/query '{"query":"production db password"}'
Assert "tenant isolation still holds after restart" ($t42b -notmatch 'hunter2')

# 8. Hold open for browser verification
Log ""
Log "== 8/8 Holding server open 300s for live browser verification =="
@{ base = $base; writeKey = $writeKey; adminKey = $adminKey; pid = $server.Id; until = (Get-Date).AddSeconds(300).ToString('o') } |
  ConvertTo-Json | Set-Content $sessionFile
Log ""
Log ("RESULT: {0} passed, {1} failed" -f $script:pass, $script:fail)
Log "Server stays up at $base for 300s, then cleans up."
Start-Sleep -Seconds 300

Stop-Process -Id $server.Id -Force -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $dataDir -ErrorAction SilentlyContinue
Remove-Item $sessionFile -ErrorAction SilentlyContinue
Log "cleanup done — server stopped, temp data removed."
