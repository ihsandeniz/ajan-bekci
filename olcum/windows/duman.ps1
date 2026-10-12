# ajan-bekci — Windows duman ölçümü (CI windows-latest'te koşar; gerçek Windows makinesi yok).
# Sahte ajan tests/veri/sahte-ajan.js: MCP benzeri torun (+ çocuğu), `start /b` torunu, detached torun.
# Her senaryo yalnız kendi PID dosyalarındaki süreçlere bakar; temizlik yalnız onlar.
#   S1 normal çıkış (kod 5)            → kod 5 aktarılır, torun 0
#   S2 bekçiye TerminateProcess        → torun 0 (KILL_ON_JOB_CLOSE)
#   S3 iç içe iş, dış bekçi öldürülür  → iç bekçi + torun 0
#   S4 iç içe normal çıkış (kod 9)     → kod 9 iki kattan geçer, torun 0
# Çıktı: <Is>/duman.tsv (tur, senaryo, kod, kalan, sonuc). Herhangi biri KIRMIZI → çıkış 1.
param(
  [int]$Tur = 3,
  [Parameter(Mandatory = $true)][string]$Bekci,
  [Parameter(Mandatory = $true)][string]$Is
)
$ErrorActionPreference = 'Stop'
$Bekci = (Resolve-Path $Bekci).Path
$Ajan = (Resolve-Path (Join-Path $PSScriptRoot '..\..\tests\veri\sahte-ajan.js')).Path
New-Item -ItemType Directory -Force -Path $Is | Out-Null
$Tsv = Join-Path $Is 'duman.tsv'
"tur`tsenaryo`tkod`tkalan`tsonuc" | Set-Content -Encoding utf8 $Tsv
$Torunlar = '.ajan', '.mcp', '.mcp.arac', '.sb', '.ayrik'
$kirmizi = 0

function Pidler($onek) {
  $son = (Get-Date).AddSeconds(20)
  while ((Get-Date) -lt $son) {
    if (@($Torunlar | Where-Object { -not (Test-Path "$onek$_") }).Count -eq 0) { break }
    Start-Sleep -Milliseconds 100
  }
  Start-Sleep -Milliseconds 200
  $Torunlar | ForEach-Object { if (Test-Path "$onek$_") { [int](Get-Content "$onek$_").Trim() } }
}

function Kalan($pidler) {
  $son = (Get-Date).AddSeconds(10)
  do {
    $k = @($pidler | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
    if ($k.Count -eq 0) { return 0 }
    Start-Sleep -Milliseconds 200
  } while ((Get-Date) -lt $son)
  return $k.Count
}

function Supur($pidler) {
  foreach ($p in $pidler) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }
}

function Yaz($t, $s, $kod, $kalan, $beklenenKod) {
  $iyi = ($kalan -eq 0) -and ($null -eq $beklenenKod -or $kod -eq $beklenenKod)
  if (-not $iyi) { $script:kirmizi++ }
  $sonuc = if ($iyi) { 'YESIL' } else { 'KIRMIZI' }
  "$t`t$s`t$kod`t$kalan`t$sonuc" | Add-Content -Encoding utf8 $Tsv
  Write-Host "tur $t $s kod=$kod kalan=$kalan $sonuc"
}

for ($t = 1; $t -le $Tur; $t++) {
  # S1 normal çıkış
  $o = Join-Path $Is "t$t-s1"
  $p = Start-Process -FilePath $Bekci -ArgumentList @('--', 'node', "`"$Ajan`"", "`"$o`"", 'cik', '5') -NoNewWindow -PassThru -Wait
  $pp = Pidler $o; $k = Kalan $pp; Supur $pp
  Yaz $t 'S1-normal-cikis' $p.ExitCode $k 5

  # S2 bekçiye TerminateProcess
  $o = Join-Path $Is "t$t-s2"
  $p = Start-Process -FilePath $Bekci -ArgumentList @('--', 'node', "`"$Ajan`"", "`"$o`"", 'bekle') -NoNewWindow -PassThru
  $pp = Pidler $o
  Stop-Process -Id $p.Id -Force   # TerminateProcess
  $k = Kalan $pp; Supur $pp
  Yaz $t 'S2-bekciye-terminate' '-' $k $null

  # S3 iç içe, dış bekçi öldürülür
  $o = Join-Path $Is "t$t-s3"
  $p = Start-Process -FilePath $Bekci -ArgumentList @('--', "`"$Bekci`"", '--', 'node', "`"$Ajan`"", "`"$o`"", 'bekle') -NoNewWindow -PassThru
  $pp = Pidler $o
  $ic = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)" | ForEach-Object { [int]$_.ProcessId })
  Stop-Process -Id $p.Id -Force
  $k = Kalan ($pp + $ic); Supur ($pp + $ic)
  Yaz $t 'S3-ic-ice-dis-terminate' '-' $k $null

  # S4 iç içe normal çıkış
  $o = Join-Path $Is "t$t-s4"
  $p = Start-Process -FilePath $Bekci -ArgumentList @('--', "`"$Bekci`"", '--', 'node', "`"$Ajan`"", "`"$o`"", 'cik', '9') -NoNewWindow -PassThru -Wait
  $pp = Pidler $o; $k = Kalan $pp; Supur $pp
  Yaz $t 'S4-ic-ice-kod' $p.ExitCode $k 9
}

Write-Host "kırmızı: $kirmizi"
if ($kirmizi -gt 0) { exit 1 }
