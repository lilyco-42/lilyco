function Test-InstalledShortcutIcon {
    param([string]$Location, [string]$Target, [string]$ExpectedBinary)
    # WSH may expose the implicit target icon as an empty path plus ',0'.
    $iconPath = $Location.Trim()
    $index = 0
    if ($iconPath -match '^(.*),\s*(-?\d+)\s*$') {
        $iconPath = $Matches[1].Trim()
        $index = [int]$Matches[2]
    }
    if ($iconPath.Length -ge 2 -and $iconPath.StartsWith('"') -and $iconPath.EndsWith('"')) {
        $iconPath = $iconPath.Substring(1,$iconPath.Length - 2)
    }
    if (-not $iconPath) { $iconPath = $Target }
    if ($index -ne 0 -or -not [IO.Path]::IsPathFullyQualified($iconPath)) { return $false }
    return [IO.Path]::GetFullPath($iconPath).Equals([IO.Path]::GetFullPath($ExpectedBinary),[StringComparison]::OrdinalIgnoreCase)
}

function Test-InstalledLocation {
    param([string]$Location, [string]$ExpectedPath)
    if ([string]::IsNullOrWhiteSpace($Location)) { return $false }
    $value = $Location.Trim()
    if ($value.Length -ge 2 -and $value.StartsWith('"') -and $value.EndsWith('"')) {
        $value = $value.Substring(1,$value.Length - 2)
    }
    if ($value.Contains('"') -or -not [IO.Path]::IsPathFullyQualified($value)) { return $false }
    return [IO.Path]::GetFullPath($value).TrimEnd('\','/').Equals([IO.Path]::GetFullPath($ExpectedPath).TrimEnd('\','/'),[StringComparison]::OrdinalIgnoreCase)
}
