<#
.SYNOPSIS
  Generates a deterministic, disposable benchmark corpus (or verifies one).
.DESCRIPTION
  The corpus lives in an owned folder marked with .perf-fixture. The same Size and Seed
  always produce the same tree, names, sizes and bytes. A perf-manifest.json records
  counts, bytes and a path|size fingerprint (the manifest itself is excluded).
.EXAMPLE
  powershell -File scripts/perf/new-corpus.ps1 -Size small
  powershell -File scripts/perf/new-corpus.ps1 -Size large -CorpusParent C:\perf
  powershell -File scripts/perf/new-corpus.ps1 -Verify -Root .gg\perf-corpus\small
#>
[CmdletBinding()]
param(
  [ValidateSet("small", "medium", "large")][string]$Size = "small",
  [int]$Seed = 20260925,
  [string]$CorpusParent,
  [string]$Root,
  [switch]$Force,
  [switch]$Verify
)
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Text;

public sealed class PerfCorpusCounts
{
    public long Files; public long Directories; public long Bytes;
    public long TreeFiles; public long LargeFiles; public long DuplicateGroups; public long DuplicateFiles;
    public long EmptyDirectories; public long Projects; public long ArtifactFiles; public long ArtifactBytes;
}

public static class PerfCorpus
{
    const string Manifest = "perf-manifest.json";
    const string Marker = ".perf-fixture";

    static void Fill(Random random, byte[] buffer, int count)
    {
        if (count == buffer.Length) { random.NextBytes(buffer); return; }
        byte[] part = new byte[count];
        random.NextBytes(part);
        Buffer.BlockCopy(part, 0, buffer, 0, count);
    }

    static void WriteFile(string path, Random random, long size, byte[] chunk, PerfCorpusCounts counts)
    {
        using (FileStream stream = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None, 1 << 16))
        {
            long remaining = size;
            while (remaining > 0)
            {
                int count = (int)Math.Min(remaining, chunk.Length);
                Fill(random, chunk, count);
                stream.Write(chunk, 0, count);
                remaining -= count;
            }
        }
        counts.Files++; counts.Bytes += size;
    }

    static string Dir(string path, PerfCorpusCounts counts)
    {
        Directory.CreateDirectory(path); counts.Directories++; return path;
    }

    static void Tree(string dir, int depth, int fanout, int filesPerDir, int maxTiny, Random random, byte[] chunk, PerfCorpusCounts counts)
    {
        for (int f = 0; f < filesPerDir; f++)
        {
            WriteFile(Path.Combine(dir, "f" + f.ToString("D4") + ".dat"), random, random.Next(0, maxTiny + 1), chunk, counts);
            counts.TreeFiles++;
        }
        if (depth == 0) return;
        for (int d = 0; d < fanout; d++)
            Tree(Dir(Path.Combine(dir, "d" + d.ToString("D2")), counts), depth - 1, fanout, filesPerDir, maxTiny, random, chunk, counts);
    }

    public static PerfCorpusCounts Generate(string root, int seed, int fanout, int depth, int filesPerDir, int maxTiny,
        int largeCount, long largeBytes, int dupGroups, int dupCopies, int dupBytes, int emptyDirs, int projects, int artifactFiles)
    {
        PerfCorpusCounts counts = new PerfCorpusCounts();
        Random random = new Random(seed);
        byte[] chunk = new byte[1 << 20];
        Tree(Dir(Path.Combine(root, "tree"), counts), depth, fanout, filesPerDir, maxTiny, random, chunk, counts);

        string large = Dir(Path.Combine(root, "large"), counts);
        for (int i = 0; i < largeCount; i++)
        {
            WriteFile(Path.Combine(large, "large" + i.ToString("D2") + ".bin"), random, largeBytes, chunk, counts);
            counts.LargeFiles++;
        }

        string dups = Dir(Path.Combine(root, "dups"), counts);
        byte[] content = new byte[dupBytes];
        for (int g = 0; g < dupGroups; g++)
        {
            random.NextBytes(content);
            string group = Dir(Path.Combine(dups, "g" + g.ToString("D4")), counts);
            for (int c = 0; c < dupCopies; c++)
            {
                File.WriteAllBytes(Path.Combine(group, "copy" + c + ".bin"), content);
                counts.Files++; counts.Bytes += dupBytes; counts.DuplicateFiles++;
            }
            counts.DuplicateGroups++;
        }

        string empty = Dir(Path.Combine(root, "empty"), counts);
        for (int e = 0; e < emptyDirs; e++)
        {
            string outer = Dir(Path.Combine(empty, "e" + e.ToString("D4")), counts);
            if (e % 2 == 0) Dir(Path.Combine(outer, "nested"), counts);
            counts.EmptyDirectories++;
        }

        string projectRoot = Dir(Path.Combine(root, "projects"), counts);
        for (int p = 0; p < projects; p++)
        {
            bool rust = p % 2 == 0;
            string project = Dir(Path.Combine(projectRoot, (rust ? "rust" : "node") + p.ToString("D3")), counts);
            string marker = rust ? "Cargo.toml" : "package.json";
            string markerText = rust ? "[package]\nname = \"perf" + p + "\"\n" : "{\"name\":\"perf" + p + "\"}\n";
            File.WriteAllText(Path.Combine(project, marker), markerText);
            counts.Files++; counts.Bytes += Encoding.UTF8.GetByteCount(markerText);
            string artifact = Dir(Path.Combine(project, rust ? "target" : "node_modules"), counts);
            string sub = Dir(Path.Combine(artifact, rust ? "debug" : "pkg"), counts);
            for (int a = 0; a < artifactFiles; a++)
            {
                long before = counts.Bytes;
                WriteFile(Path.Combine(sub, "a" + a.ToString("D4") + ".o"), random, random.Next(512, 16385), chunk, counts);
                counts.ArtifactFiles++; counts.ArtifactBytes += counts.Bytes - before;
            }
            counts.Projects++;
        }
        return counts;
    }

    // Fingerprint of sorted "relative-path|size" lines; ignores the manifest and marker.
    public static string Fingerprint(string root, out long files, out long bytes)
    {
        List<string> lines = new List<string>();
        files = 0; bytes = 0;
        foreach (string path in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
        {
            string relative = path.Substring(root.Length).TrimStart('\\');
            if (relative == Manifest || relative == Marker) continue;
            long size = new FileInfo(path).Length;
            lines.Add(relative.Replace('\\', '/') + "|" + size);
            files++; bytes += size;
        }
        lines.Sort(StringComparer.Ordinal);
        using (SHA256 sha = SHA256.Create())
        {
            byte[] hash = sha.ComputeHash(Encoding.UTF8.GetBytes(string.Join("\n", lines)));
            StringBuilder text = new StringBuilder();
            foreach (byte b in hash) text.Append(b.ToString("x2"));
            return text.ToString();
        }
    }
}
"@

$profiles = @{
  # fanout/depth/filesPerDir define the wide/deep tree of tiny files.
  small  = @{ fanout = 4; depth = 3; filesPerDir = 96; maxTiny = 4096; largeCount = 2; largeBytes = 16MB; dupGroups = 20; dupCopies = 3; dupBytes = 64KB; emptyDirs = 50; projects = 10; artifactFiles = 50 }
  medium = @{ fanout = 6; depth = 4; filesPerDir = 120; maxTiny = 4096; largeCount = 4; largeBytes = 128MB; dupGroups = 200; dupCopies = 3; dupBytes = 64KB; emptyDirs = 500; projects = 50; artifactFiles = 200 }
  large  = @{ fanout = 8; depth = 5; filesPerDir = 25; maxTiny = 4096; largeCount = 8; largeBytes = 256MB; dupGroups = 1000; dupCopies = 3; dupBytes = 64KB; emptyDirs = 2000; projects = 100; artifactFiles = 500 }
}

function Get-CorpusRoot {
  if ($Root) { return [IO.Path]::GetFullPath($Root) }
  $parent = $CorpusParent
  if (-not $parent) { $parent = Join-Path (Get-PerfRepoRoot) ".gg\perf-corpus" }
  return [IO.Path]::GetFullPath((Join-Path $parent $Size))
}

$corpus = Get-CorpusRoot
$manifestPath = Join-Path $corpus "perf-manifest.json"

if ($Verify) {
  Assert-PerfFixture -Path $corpus
  $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
  $files = [long]0; $bytes = [long]0
  $fingerprint = [PerfCorpus]::Fingerprint($corpus, [ref]$files, [ref]$bytes)
  $ok = ($fingerprint -eq $manifest.fingerprint) -and ($files -eq $manifest.counts.files) -and ($bytes -eq $manifest.counts.bytes)
  Write-Output (ConvertTo-Json -Compress ([ordered]@{ root = $corpus; ok = $ok; files = $files; bytes = $bytes; fingerprint = $fingerprint }))
  if (-not $ok) { exit 1 }
  exit 0
}

if (Test-Path -LiteralPath $corpus) {
  Assert-PerfFixture -Path $corpus
  if ((Test-Path -LiteralPath $manifestPath) -and -not $Force) {
    $existing = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($existing.size -eq $Size -and $existing.seed -eq $Seed) {
      Write-Output "Corpus already present: $corpus (use -Force to regenerate)"
      exit 0
    }
  }
  Remove-PerfFixture -Path $corpus
}

$corpus = New-PerfFixtureDirectory -Path $corpus
$p = $profiles[$Size]
$watch = [Diagnostics.Stopwatch]::StartNew()
$counts = [PerfCorpus]::Generate($corpus, $Seed, $p.fanout, $p.depth, $p.filesPerDir, $p.maxTiny, $p.largeCount,
  [long]$p.largeBytes, $p.dupGroups, $p.dupCopies, [int]$p.dupBytes, $p.emptyDirs, $p.projects, $p.artifactFiles)
$watch.Stop()
$files = [long]0; $bytes = [long]0
$fingerprint = [PerfCorpus]::Fingerprint($corpus, [ref]$files, [ref]$bytes)
if ($files -ne $counts.Files -or $bytes -ne $counts.Bytes) { throw "Corpus count mismatch after generation." }

Write-PerfJson -Path $manifestPath -Value ([ordered]@{
  schemaVersion = 1
  size = $Size
  seed = $Seed
  generatorProfile = $p
  root = $corpus
  fingerprint = $fingerprint
  generationMs = [Math]::Round($watch.Elapsed.TotalMilliseconds)
  volume = Get-PerfVolumeInfo -Path $corpus
  counts = [ordered]@{
    files = $counts.Files; directories = $counts.Directories; entries = $counts.Files + $counts.Directories; bytes = $counts.Bytes
    treeFiles = $counts.TreeFiles; largeFiles = $counts.LargeFiles; duplicateGroups = $counts.DuplicateGroups
    duplicateFiles = $counts.DuplicateFiles; emptyDirectories = $counts.EmptyDirectories; projects = $counts.Projects
    artifactFiles = $counts.ArtifactFiles; artifactBytes = $counts.ArtifactBytes
  }
})
Write-Output "Corpus ready: $corpus ($($counts.Files + $counts.Directories) entries, $([Math]::Round($counts.Bytes / 1MB)) MiB, $([Math]::Round($watch.Elapsed.TotalSeconds, 1)) s)"
