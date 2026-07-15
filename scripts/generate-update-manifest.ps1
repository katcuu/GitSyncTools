[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^v?\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$')]
    [string]$Version,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^https?://')]
    [string]$WindowsUrl,

    [Parameter(Mandatory = $true)]
    [string]$WindowsSignaturePath,

    [ValidatePattern('^https?://')]
    [string]$MacArm64Url,

    [string]$MacArm64SignaturePath,

    [string]$Notes = "GitSyncTools $Version",

    [Parameter(Mandatory = $true)]
    [string]$OutputPath
)

$ErrorActionPreference = 'Stop'

function Read-Signature([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "签名文件不存在：$Path"
    }
    $signature = (Get-Content -LiteralPath $Path -Raw).Trim()
    if ([string]::IsNullOrWhiteSpace($signature)) {
        throw "签名文件为空：$Path"
    }
    return $signature
}

if ([bool]$MacArm64Url -xor [bool]$MacArm64SignaturePath) {
    throw 'MacArm64Url 和 MacArm64SignaturePath 必须同时提供'
}

$platforms = [ordered]@{
    'windows-x86_64' = [ordered]@{
        signature = Read-Signature $WindowsSignaturePath
        url = $WindowsUrl
    }
}

if ($MacArm64Url) {
    $platforms['darwin-aarch64'] = [ordered]@{
        signature = Read-Signature $MacArm64SignaturePath
        url = $MacArm64Url
    }
}

$manifest = [ordered]@{
    version = $Version.TrimStart('v')
    notes = $Notes
    pub_date = [DateTime]::UtcNow.ToString('o')
    platforms = $platforms
}

$outputFullPath = [IO.Path]::GetFullPath($OutputPath)
$parent = [IO.Path]::GetDirectoryName($outputFullPath)
if ($parent) {
    [IO.Directory]::CreateDirectory($parent) | Out-Null
}
$json = $manifest | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText($outputFullPath, $json, [Text.UTF8Encoding]::new($false))
Write-Host "已生成更新清单：$OutputPath"
