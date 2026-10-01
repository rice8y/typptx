<#
Run only on a dedicated Windows desktop with licensed PowerPoint installed.
This is an opt-in test runner, not a renderer fallback for typptx.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Presentation,
    [Parameter(Mandatory = $true)][string]$Manifest,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne 'Win32NT') {
    throw 'This runner requires Windows and the PowerPoint desktop application.'
}
if (Get-Process POWERPNT -ErrorAction SilentlyContinue) {
    throw 'Close PowerPoint before running this dedicated test. Existing sessions are never reused or closed.'
}
$sourcePath = (Resolve-Path -LiteralPath $Presentation).Path
$manifestPath = (Resolve-Path -LiteralPath $Manifest).Path
$spec = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($spec.schema_version -ne 1 -or $spec.edits.Count -lt 1) {
    throw 'The manifest must use schema version 1 and declare a native cell edit.'
}
$destination = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $destination) {
    throw 'Choose a new output directory; prior evidence is never overwritten.'
}
[void](New-Item -ItemType Directory -Path $destination)
$inputCopy = Join-Path $destination 'input.pptx'
$pdfPath = Join-Path $destination 'powerpoint.pdf'
$roundtrip = Join-Path $destination 'roundtrip.pptx'
$roundtripPdf = Join-Path $destination 'roundtrip.pdf'
$edited = Join-Path $destination 'edited.pptx'
$editedPdf = Join-Path $destination 'edited.pdf'
Copy-Item -LiteralPath $sourcePath -Destination $inputCopy

function Get-TableShapes($Shapes) {
    for ($i = 1; $i -le $Shapes.Count; $i++) {
        $shape = $Shapes.Item($i)
        if ($shape.HasTable -eq -1) {
            Write-Output -NoEnumerate $shape
        } elseif ($shape.Type -eq 6) { # msoGroup
            Get-TableShapes $shape.GroupItems
        }
    }
}

function Export-LocalPdf($Deck, [string]$Path) {
    # PDF, print quality, slides, include hidden slides, all slides. PrintRange
    # is a required nullable COM argument. Avoid the external/online exporter.
    $Deck.ExportAsFixedFormat($Path, 2, 2, 0, 1, 1, -1, $null, 1, '', $true, $true, $true, $false, $false)
}

function Close-TestDeck($Deck) {
    if ($null -ne $Deck) {
        $Deck.Saved = -1
        $Deck.Close()
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($Deck)
    }
}

$application = $null
$deck = $null
try {
    $application = New-Object -ComObject PowerPoint.Application
    $application.Visible = -1
    $version = $application.Version
    $process = Get-Process POWERPNT | Select-Object -First 1
    $build = (Get-Item -LiteralPath $process.Path).VersionInfo.ProductVersion
    $deck = $application.Presentations.Open($inputCopy, -1, 0, -1)
    if ($deck.Slides.Count -ne $spec.slides) { throw 'Unexpected slide count after opening.' }
    Export-LocalPdf $deck $pdfPath
    $deck.SaveCopyAs($roundtrip, 24, -2) # ppSaveAsOpenXMLPresentation, preserve font embedding
    Close-TestDeck $deck
    $deck = $null

    $deck = $application.Presentations.Open($roundtrip, 0, 0, -1)
    Export-LocalPdf $deck $roundtripPdf
    foreach ($edit in $spec.edits) {
        $tables = @(Get-TableShapes ($deck.Slides.Item([int]$edit.slide).Shapes))
        if ($edit.table -lt 1 -or $edit.table -gt $tables.Count) { throw 'The requested native table is missing.' }
        $cell = $tables[[int]$edit.table - 1].Table.Cell([int]$edit.row, [int]$edit.column)
        $actual = $cell.Shape.TextFrame.TextRange.Text.Replace("`r", "`n").Replace([string][char]11, "`n")
        if ($actual -cne $edit.before) { throw "Cell edit precondition failed: '$actual' != '$($edit.before)'." }
        $cell.Shape.TextFrame.TextRange.Text = $edit.after.Replace("`n", "`r")
    }
    $deck.SaveCopyAs($edited, 24, -2)
    Close-TestDeck $deck
    $deck = $null

    $deck = $application.Presentations.Open($edited, -1, 0, -1)
    foreach ($edit in $spec.edits) {
        $tables = @(Get-TableShapes ($deck.Slides.Item([int]$edit.slide).Shapes))
        $actual = $tables[[int]$edit.table - 1].Table.Cell([int]$edit.row, [int]$edit.column).Shape.TextFrame.TextRange.Text.Replace("`r", "`n").Replace([string][char]11, "`n")
        if ($actual -cne $edit.after) { throw 'The edited cell did not survive reopening.' }
    }
    Export-LocalPdf $deck $editedPdf
    $artifacts = [ordered]@{}
    foreach ($entry in @{
        pptx = $inputCopy; pdf = $pdfPath; roundtrip_pptx = $roundtrip;
        edited_pptx = $edited; roundtrip_pdf = $roundtripPdf; edited_pdf = $editedPdf
    }.GetEnumerator()) {
        $artifacts[$entry.Key] = @{ path = $entry.Value; sha256 = (Get-FileHash -LiteralPath $entry.Value -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
    $evidence = [ordered]@{
        application = 'Microsoft PowerPoint'; version = "$version (build $build)"
        os = [Environment]::OSVersion.VersionString
        export_method = 'ExportAsFixedFormat'
        recorded_utc = [DateTime]::UtcNow.ToString('o')
        steps = @('open', 'local PDF export', 'save copy', 'close', 'reopen', 'edit native cell', 'save copy', 'close', 'reopen', 'verify cell text', 'local PDF export')
        artifacts = $artifacts
    }
    $evidence | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $destination 'office-evidence.json') -Encoding UTF8
} finally {
    try {
        Close-TestDeck $deck
    } finally {
        if ($null -ne $application) {
            $application.Quit()
            [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($application)
        }
        [GC]::Collect()
        [GC]::WaitForPendingFinalizers()
    }
}
