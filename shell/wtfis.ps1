# Dot-source this file from your PowerShell profile to enable parent-shell cd.

function global:Set-Location {
    [CmdletBinding(DefaultParameterSetName = "Path")]
    param(
        [Parameter(Position = 0, ParameterSetName = "Path")]
        [string] $Path,
        [Parameter(Position = 0, ParameterSetName = "LiteralPath")]
        [string] $LiteralPath,
        [switch] $PassThru
    )

    $previous = (Microsoft.PowerShell.Management\Get-Location).Path
    try {
        if ($PSCmdlet.ParameterSetName -eq "LiteralPath") {
            Microsoft.PowerShell.Management\Set-Location -LiteralPath $LiteralPath -PassThru:$PassThru
        } else {
            Microsoft.PowerShell.Management\Set-Location -Path $Path -PassThru:$PassThru
        }
        $env:WTFIS_PREV_CD = $previous
        Remove-Item Env:WTFIS_LAST_CD -ErrorAction SilentlyContinue
    } catch {
        if ($PSCmdlet.ParameterSetName -eq "Path" -and $Path) {
            $env:WTFIS_LAST_CD = $Path
            Write-Host "Try: wtfis --up" -ForegroundColor Yellow
        }
        throw
    }
}

function global:wtfis {
    param(
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]] $Arguments
    )

    if ($Arguments.Count -eq 1 -and $Arguments[0] -eq "--home") {
        $previous = (Microsoft.PowerShell.Management\Get-Location).Path
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $HOME
        $env:WTFIS_PREV_CD = $previous
        Remove-Item Env:WTFIS_LAST_CD -ErrorAction SilentlyContinue
        return
    }

    if ($Arguments.Count -eq 1 -and $Arguments[0] -eq "--prev") {
        if (-not $env:WTFIS_PREV_CD) {
            Write-Error "wtfis: no previous directory is available"
            return
        }
        $previous = (Microsoft.PowerShell.Management\Get-Location).Path
        Microsoft.PowerShell.Management\Set-Location -LiteralPath $env:WTFIS_PREV_CD
        $env:WTFIS_PREV_CD = $previous
        Remove-Item Env:WTFIS_LAST_CD -ErrorAction SilentlyContinue
        return
    }

    $binary = Get-Command wtfis.exe -CommandType Application -ErrorAction Stop
    $output = [System.IO.Path]::GetTempFileName()
    $previous = (Microsoft.PowerShell.Management\Get-Location).Path
    $oldOutput = $env:WTFIS_OUTPUT
    $env:WTFIS_OUTPUT = $output

    try {
        & $binary.Source @Arguments
        $status = $LASTEXITCODE
    } finally {
        if ($null -eq $oldOutput) {
            Remove-Item Env:WTFIS_OUTPUT -ErrorAction SilentlyContinue
        } else {
            $env:WTFIS_OUTPUT = $oldOutput
        }
    }

    if ($status -eq 0 -and (Test-Path -LiteralPath $output)) {
        $lines = @(Get-Content -LiteralPath $output)
        if ($lines.Count -gt 0 -and $lines[0]) {
            $selectedPath = $lines[0]
            $selectedCommand = if ($lines.Count -gt 1) {
                ($lines[1..($lines.Count - 1)] -join [Environment]::NewLine)
            } else {
                ""
            }

            if ($Arguments.Count -eq 1 -and $Arguments[0] -eq "--where") {
                Write-Output $selectedPath
            } else {
                Microsoft.PowerShell.Management\Set-Location -LiteralPath $selectedPath
                $env:WTFIS_PREV_CD = $previous
                Remove-Item Env:WTFIS_LAST_CD -ErrorAction SilentlyContinue
                if ($selectedCommand) {
                    Invoke-Expression $selectedCommand
                    $status = $LASTEXITCODE
                }
            }
        }
    }

    Remove-Item -LiteralPath $output -Force -ErrorAction SilentlyContinue
    if ($null -ne $status) {
        $global:LASTEXITCODE = $status
    }
}

function global:cdd {
    param(
        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]] $Arguments
    )
    wtfis @Arguments
}
