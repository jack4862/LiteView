# Benchmarks

`packaging\run-benchmarks.ps1` builds the release package, runs the bundled benchmark command over
every supported image in a local fixture directory, and writes a machine-readable JSON report plus
a Markdown summary.

Do not commit personal photos. A useful local fixture directory contains one 20MP JPEG, one
108MP JPEG, one very wide image, one very tall image, and one 16-bit PNG. Run:

```powershell
.\packaging\run-benchmarks.ps1 -ImagesDirectory "C:\path\to\liteview-benchmarks" -Iterations 20
```

`benchmark-results.json` preserves image paths, dimensions, ICC status, and elapsed-time percentiles.
`benchmark-results.md` is a compact review summary. Both files are local generated artifacts and are
ignored by Git; do not commit them or personal image paths.
