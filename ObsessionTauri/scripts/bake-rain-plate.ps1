# Бейк подложки темы Rain из апскейленного кадра ночной улицы.
#
# Делает два ассета:
#   world-plate.jpg     — кадр 16:9, 2048x1152 (шире 1080p: параллакс сдвигает
#                         плиту и должен открывать реальные пиксели, а не край);
#   world-emission.png  — карта эмиссии: только яркие ядра фонарей в их родном
#                         цвете. Шейдер поднимает её до HDR-яркости, чтобы капли
#                         собирали настоящее боке.
#
# Рамка: окно 2944x1656 при y=880 в исходном квадрате 2944x2944 — держит ядро
# главного фонаря, коридор деревьев, дальние огни со знаком и полосу мокрого
# асфальта снизу.
#
# Запуск:
#   powershell -ExecutionPolicy Bypass -File scripts/bake-rain-plate.ps1 `
#     -Source "C:\path\01-opa.jpg"

param(
  [string]$Source = "$env:USERPROFILE\Downloads\New folder\01-opa.jpg",
  [string]$OutDir = (Join-Path $PSScriptRoot "..\public\rain"),
  [int]$CropY = 880,
  [int]$CropHeight = 1656,
  [int]$PlateWidth = 2048,
  [int]$EmissionWidth = 1024,
  [double]$EmissionThreshold = 0.60,
  [double]$EmissionKnee = 0.35,
  [int]$PlateQuality = 88
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
# Иначе кириллица в выводе уезжает в OEM-кодировку консоли.
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

if (-not (Test-Path $Source)) { throw "Нет исходника: $Source" }
$OutDir = (Resolve-Path $OutDir).Path

function Resize-Bitmap([System.Drawing.Image]$image, [int]$width, [int]$height) {
  $target = New-Object System.Drawing.Bitmap $width, $height
  $graphics = [System.Drawing.Graphics]::FromImage($target)
  $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $graphics.DrawImage($image, 0, 0, $width, $height)
  $graphics.Dispose()
  return $target
}

function Save-Jpeg([System.Drawing.Bitmap]$bitmap, [string]$path, [int]$quality) {
  $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() |
    Where-Object { $_.MimeType -eq "image/jpeg" }
  $params = New-Object System.Drawing.Imaging.EncoderParameters 1
  $params.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter(
    [System.Drawing.Imaging.Encoder]::Quality, [int64]$quality)
  $bitmap.Save($path, $codec, $params)
  $params.Dispose()
}

# Имя локальной переменной не должно совпадать с параметром $Source: параметр
# объявлен как [string], и присваивание Bitmap молча превратилось бы в строку.
$image = New-Object System.Drawing.Bitmap($Source)
"источник: $($image.Width)x$($image.Height)"
$rect = New-Object System.Drawing.Rectangle 0, $CropY, $image.Width, $CropHeight
$crop = $image.Clone($rect, $image.PixelFormat)
$image.Dispose()

$plateHeight = [int][Math]::Round($PlateWidth * $CropHeight / $crop.Width)
$plate = Resize-Bitmap $crop $PlateWidth $plateHeight
$platePath = Join-Path $OutDir "world-plate.jpg"
Save-Jpeg $plate $platePath $PlateQuality
"плита: $($plate.Width)x$($plate.Height) -> $platePath ($([Math]::Round((Get-Item $platePath).Length / 1KB, 1)) KB)"

# Эмиссия считается с уменьшенной копии: ядра фонарей и так уйдут в блум, а
# 0.6 Мп вместо 2.4 Мп заметно ускоряют проход.
$emissionHeight = [int][Math]::Round($EmissionWidth * $CropHeight / $crop.Width)
$small = Resize-Bitmap $crop $EmissionWidth $emissionHeight
$crop.Dispose()

$readRect = New-Object System.Drawing.Rectangle 0, 0, $small.Width, $small.Height
$readData = $small.LockBits($readRect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
  [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$readBytes = New-Object byte[] ($readData.Stride * $small.Height)
[System.Runtime.InteropServices.Marshal]::Copy($readData.Scan0, $readBytes, 0, $readBytes.Length)
$small.UnlockBits($readData)

$emission = New-Object System.Drawing.Bitmap $small.Width, $small.Height,
  ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$writeData = $emission.LockBits($readRect, [System.Drawing.Imaging.ImageLockMode]::WriteOnly,
  [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$writeBytes = New-Object byte[] ($writeData.Stride * $emission.Height)

$hot = 0
for ($y = 0; $y -lt $small.Height; $y++) {
  $readRow = $y * $readData.Stride
  $writeRow = $y * $writeData.Stride
  for ($x = 0; $x -lt $small.Width; $x++) {
    $i = $readRow + $x * 3
    $b = $readBytes[$i]; $g = $readBytes[$i + 1]; $r = $readBytes[$i + 2]
    $luma = (0.299 * $r + 0.587 * $g + 0.114 * $b) / 255.0
    $mask = ($luma - $EmissionThreshold) / $EmissionKnee
    if ($mask -le 0) { $mask = 0 } elseif ($mask -ge 1) { $mask = 1 }
    # Мягкое колено: слабые засветки не должны попадать в источники света.
    $mask = [Math]::Pow($mask, 1.4)
    if ($mask -gt 0.02) { $hot++ }
    $o = $writeRow + $x * 4
    $writeBytes[$o] = [byte][Math]::Round($b * $mask)
    $writeBytes[$o + 1] = [byte][Math]::Round($g * $mask)
    $writeBytes[$o + 2] = [byte][Math]::Round($r * $mask)
    $writeBytes[$o + 3] = 255
  }
}
[System.Runtime.InteropServices.Marshal]::Copy($writeBytes, 0, $writeData.Scan0, $writeBytes.Length)
$emission.UnlockBits($writeData)
$small.Dispose()

$emissionPath = Join-Path $OutDir "world-emission.png"
$emission.Save($emissionPath, [System.Drawing.Imaging.ImageFormat]::Png)
$total = $emission.Width * $emission.Height
"эмиссия: $($emission.Width)x$($emission.Height) -> $emissionPath ($([Math]::Round((Get-Item $emissionPath).Length / 1KB, 1)) KB)"
"источников света: $([Math]::Round(100.0 * $hot / $total, 3))% пикселей"
$emission.Dispose()
$plate.Dispose()
