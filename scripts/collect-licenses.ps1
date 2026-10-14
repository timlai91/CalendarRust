[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $destination = Join-Path $root 'licenses\third-party'
    $indexPath = Join-Path $destination 'index.json'
    $lockHash = (Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant()
    if (Test-Path -LiteralPath $indexPath) {
        $existing = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
        if ($existing.lock_sha256 -eq $lockHash) {
            foreach ($package in $existing.packages) {
                foreach ($file in $package.files) {
                    $path = Join-Path $destination $file.path
                    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $file.sha256) {
                        throw "Third-party notice hash mismatch: $($file.path)"
                    }
                }
            }
            Write-Output "Verified offline third-party notices: $($existing.packages.Count) packages"
            return
        }
    }
    $json = cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'cargo metadata failed' }
    $metadata = $json | ConvertFrom-Json
    $ids = [Collections.Generic.HashSet[string]]::new()
    $queue = [Collections.Generic.Queue[string]]::new()
    $queue.Enqueue($metadata.resolve.root)
    while ($queue.Count) {
        $id = $queue.Dequeue()
        if ($ids.Add($id)) {
            $node = $metadata.resolve.nodes | Where-Object id -eq $id
            foreach ($dependency in $node.deps) { $queue.Enqueue($dependency.pkg) }
        }
    }
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    $upstream = @{}
    $packages = @()
    foreach ($package in $metadata.packages | Where-Object { $ids.Contains($_.id) -and $_.source } | Sort-Object name, version) {
        if (!$package.license -and !$package.license_file) { throw "Unknown license: $($package.name)" }
        $crate = Split-Path $package.manifest_path -Parent
        $folder = "$($package.name)-$($package.version)"
        $output = Join-Path $destination $folder
        New-Item -ItemType Directory -Path $output -Force | Out-Null
        $candidates = @(Get-ChildItem -LiteralPath $crate -Recurse -File | Where-Object {
            $_.Name -match '(?i)licen[cs]e|copying|notice|copyright' -and $_.Extension -notin @('.rs','.c','.h','.png','.ttf')
        })
        if ($package.license_file) {
            $specified = [IO.Path]::GetFullPath((Join-Path $crate $package.license_file))
            if (!$specified.StartsWith("$crate\", [StringComparison]::OrdinalIgnoreCase)) { throw 'License escapes crate' }
            $candidates += Get-Item -LiteralPath $specified
        }
        $files = @()
        foreach ($file in $candidates | Sort-Object FullName -Unique) {
            $name = $file.FullName.Substring($crate.Length + 1).Replace('\', '__')
            Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $output $name)
            $files += @{path="$folder\$name"; sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant(); origin='locked crate'}
        }
        if (!$files.Count) {
            $vcsPath = Join-Path $crate '.cargo_vcs_info.json'
            $vcs = Get-Content -LiteralPath $vcsPath -Raw | ConvertFrom-Json
            $revision = $vcs.git.sha1
            if (!$revision -or $package.repository -notmatch '^https://github\.com/([^/]+/[^/]+)') {
                throw "No traceable license notice: $($package.name)"
            }
            $repository = $Matches[1] -replace '\.git$', ''
            $key = "$repository@$revision"
            if (!$upstream.ContainsKey($key)) {
                $response = gh api "repos/$repository/contents?ref=$revision"
                if ($LASTEXITCODE) { throw "Unable to retrieve notices: $key" }
                $entries = $response | ConvertFrom-Json
                $notices = @($entries | Where-Object { $_.type -eq 'file' -and $_.name -match '(?i)licen[cs]e|copying|notice|copyright' })
                if (!$notices.Count) { throw "Upstream has no root license notice: $key" }
                $upstream[$key] = @($notices | ForEach-Object {
                    $text = Invoke-WebRequest -Uri $_.download_url
                    @{name=$_.name; bytes=[Text.Encoding]::UTF8.GetBytes($text.Content); url=$_.download_url}
                })
            }
            foreach ($notice in $upstream[$key]) {
                $target = Join-Path $output $notice.name
                [IO.File]::WriteAllBytes($target, $notice.bytes)
                $files += @{path="$folder\$($notice.name)"; sha256=(Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant(); origin=$notice.url}
            }
        }
        $packages += @{name=$package.name; version=$package.version; license=$package.license; repository=$package.repository; files=$files}
    }
    $index = @{lock_sha256=$lockHash; target='x86_64-pc-windows-msvc'; packages=$packages} | ConvertTo-Json -Depth 12
    [IO.File]::WriteAllText($indexPath, $index, [Text.UTF8Encoding]::new($false))
    Write-Output "Collected third-party notices: $($packages.Count) packages"
} finally { Pop-Location }
