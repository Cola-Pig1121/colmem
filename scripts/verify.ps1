param(
    [string]$Query = "hybrid retrieval",
    [string]$HostId = "codex",
    [string]$Agent = "builder",
    [switch]$SkipIngest,
    [switch]$SkipQuery,
    [switch]$LiveBinary,
    [int]$RetryCount = 3,
    [int]$RetryDelayMs = 750
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$cliBinary = Join-Path $repoRoot "target\debug\colmem-cli.exe"

function Get-LatestSourceWriteTimeUtc {
    param(
        [string]$Root
    )

    $items = @(
        Get-ChildItem -Path (Join-Path $Root "crates") -Recurse -File -Include *.rs,Cargo.toml
        Get-Item (Join-Path $Root "Cargo.toml")
    )

    return ($items | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1).LastWriteTimeUtc
}

function Invoke-CheckedNative {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Invoke-CheckedNativeWithRetry {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$Arguments = @(),
        [Parameter(Mandatory = $true)]
        [string]$Label
    )

    for ($attempt = 1; $attempt -le $RetryCount; $attempt++) {
        try {
            Invoke-CheckedNative -FilePath $FilePath -Arguments $Arguments
            return
        }
        catch {
            if ($attempt -ge $RetryCount) {
                throw
            }

            Write-Warning "${Label} failed on attempt ${attempt}/${RetryCount}: $($_.Exception.Message)"
            Start-Sleep -Milliseconds $RetryDelayMs
        }
    }
}

Push-Location $repoRoot
try {
    Write-Host "[1/2] Running cargo test"
    Invoke-CheckedNativeWithRetry -FilePath "cargo" -Arguments @("test") -Label "cargo test"

    if (-not $LiveBinary) {
        Write-Host "[2/2] Logic verification completed through cargo test."
        Write-Host "Use -LiveBinary to require a fresh CLI/MCP binary build and runtime smoke checks."
        return
    }

    Write-Host "[2/5] Building fresh CLI and MCP binaries"
    Invoke-CheckedNativeWithRetry -FilePath "cargo" -Arguments @("build", "-p", "colmem-cli", "-p", "colmem-mcp") -Label "cargo build -p colmem-cli -p colmem-mcp"

    if (-not (Test-Path $cliBinary)) {
        throw "Missing CLI binary at $cliBinary after cargo test."
    }

    $latestSourceWriteTimeUtc = Get-LatestSourceWriteTimeUtc -Root $repoRoot
    $cliWriteTimeUtc = (Get-Item $cliBinary).LastWriteTimeUtc
    if ($cliWriteTimeUtc -lt $latestSourceWriteTimeUtc) {
        throw "Potential stale CLI binary detected. Binary time $cliWriteTimeUtc is older than latest source time $latestSourceWriteTimeUtc."
    }

    Write-Host "[3/5] Verifying host catalog through built binary"
    & $cliBinary host list

    if (-not $SkipIngest) {
        Write-Host "[4/5] Verifying ingest through built binary"
        & $cliBinary ingest
    } else {
        Write-Host "[4/5] Skipping ingest"
    }

    if (-not $SkipQuery) {
        Write-Host "[5/5] Verifying query through built binary"
        & $cliBinary query $Query $HostId $Agent
    } else {
        Write-Host "[5/5] Skipping query"
    }

    Write-Host "Verification completed successfully."
}
finally {
    Pop-Location
}
