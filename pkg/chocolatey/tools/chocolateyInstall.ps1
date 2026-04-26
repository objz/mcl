$ErrorActionPreference = 'Stop'

$packageArgs = @{
  packageName    = $env:ChocolateyPackageName
  fileType       = 'msi'
  url64bit       = 'https://github.com/objz/mcl/releases/download/__TAG__/mcl-launcher-x86_64-pc-windows-msvc.msi'
  softwareName   = 'mcl-launcher*'
  checksum64     = '__CHECKSUM64__'
  checksumType64 = 'sha256'
  silentArgs     = '/quiet /norestart'
  validExitCodes = @(0, 3010, 1641)
}

Install-ChocolateyPackage @packageArgs
