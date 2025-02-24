# Inno Setup executable path
$innoSetupPath = "C:\zjk\apps\Inno Setup 6\ISCC.exe"
$repoDir = "C:\zjk\projects\zed"
$innoFilePath = "$repoDir\crates\zed\resources\windows\installer\zed.iss"
$signToolPath = "powershell.exe -ExecutionPolicy Bypass -File $repoDir\crates\zed\resources\windows\installer\sign.ps1"

$product = @{
    "nameLong"         = "Zed"
    "nameShort"        = "zed"
    "DirName"          = "Zed"
    "RegValueName"     = "ZedEditor"
    "RegValueNameLong" = "Zed Editor (User)"
    "ShellNameShort"   = "&Zed Editor"
    "MutexName"        = "ZedSetupMutex" # TODO:
    "AppUserModelId"   = "ZedIndustry.Zed"
    "ResourcesDir"     = "$repoDir\crates\zed\resources\windows"
    ”AppId"            = "{{2DB0DA96-CA55-49BB-AF4F-64AF36A86712}"
}

$sourcePath = $repoDir
$outputPath = "$repoDir\target\windows"
New-Item -ItemType Directory -Force -Path $outputPath | Out-Null

$definitions = @{
    "NameLong"         = $product.nameLong
    "NameShort"        = $product.nameShort
    "DirName"          = $product.DirName
    "Version"          = "1.0.0"
    "RawVersion"       = "1.0.0"
    "ExeBasename"      = $product.nameShort
    "RegValueName"     = $product.RegValueName
    "RegValueNameLong" = $product.RegValueNameLong
    "ShellNameShort"   = $product.ShellNameShort
    "AppMutex"         = $product.MutexName
    "SourceDir"        = $sourcePath
    "OutputDir"        = $outputPath
    "ResourcesDir"     = $product.ResourcesDir
    "AppId"            = $product.AppId
    "AppUserId"        = $product.AppUserModelId
    "signToolPath"     = $signToolPath
}

$defs = @()
foreach ($key in $definitions.Keys) {
    $defs += "/d$key=`"$($definitions[$key])`""
}

$innoArgs = @($issPath) + $innoFilePath + $defs + "/sDefaultsign=`"$signToolPath `$f`""

# Execute Inno Setup
Write-Host "🚀 Running Inno Setup: $innoSetupPath $innoArgs"
$process = Start-Process -FilePath $innoSetupPath -ArgumentList $innoArgs -NoNewWindow -Wait -PassThru

if ($process.ExitCode -eq 0) {
    Write-Host "✅ Inno Setup successfully compiled the installer"
    exit 0
}
else {
    Write-Host "❌ Inno Setup failed: $($process.ExitCode)"
    exit $process.ExitCode
}
