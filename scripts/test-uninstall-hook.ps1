$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$makensisCommand = Get-Command makensis -ErrorAction SilentlyContinue
$makensis = if ($makensisCommand) {
  $makensisCommand.Source
} else {
  @(
    (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
    (Join-Path $env:ProgramFiles "NSIS\makensis.exe"),
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe")
  ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not (Test-Path -LiteralPath $makensis)) {
  throw "makensis not found; install NSIS or run pnpm tauri build once"
}

$testRoot = Join-Path $env:TEMP "bilidown-uninstall-hook-$PID"
$testExe = Join-Path $testRoot "test-uninstall-hook.exe"
$dataDir = Join-Path $env:TEMP "bilidown-uninstall-hook-test"
$script = Join-Path $root "scripts\test-uninstall-hook.nsi"

New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
try {
  & $makensis /INPUTCHARSET UTF8 "/DTEST_OUTFILE=$testExe" $script
  if ($LASTEXITCODE -ne 0) {
    throw "makensis failed with exit code $LASTEXITCODE"
  }

  foreach ($mode in @("delete", "keep", "update")) {
    if (Test-Path -LiteralPath $dataDir) {
      Remove-Item -LiteralPath $dataDir -Recurse -Force
    }
    $process = Start-Process -FilePath $testExe -ArgumentList "/MODE=$mode" -Wait -PassThru
    if ($process.ExitCode -ne 0) {
      throw "test executable failed for mode $mode with exit code $($process.ExitCode)"
    }

    $queue = Join-Path $dataDir "queue.sqlite"
    if ($mode -eq "delete") {
      if (Test-Path -LiteralPath $queue) {
        throw "selected uninstall did not remove the data directory"
      }
    } else {
      if (-not (Test-Path -LiteralPath $queue -PathType Leaf)) {
        throw "$mode mode unexpectedly removed the data directory"
      }
      if ((Get-Content -Raw -LiteralPath $queue) -ne "uninstall-hook-test") {
        throw "$mode mode changed queue data"
      }
    }
    Write-Output "PASS uninstall hook: $mode"
  }
} finally {
  if (Test-Path -LiteralPath $dataDir) {
    Remove-Item -LiteralPath $dataDir -Recurse -Force
  }
  if (Test-Path -LiteralPath $testRoot) {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
  }
}
