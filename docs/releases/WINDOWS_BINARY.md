# Local Windows binary package

`tools/windows-release/package.py` turns an existing Windows x86_64 executable into a local ZIP package. It does not compile, publish, install, or run the executable.

```powershell
python tools/windows-release/package.py --binary C:/build/graphhelm.exe --output C:/release/graphhelm-windows-x86_64 --source-sha <full-40-character-source-sha>
```

The output directory must not exist and its parent must already exist. The input must be a regular, non-linked file with an x86_64 PE header and a size no larger than 512 MiB. The tool stages files in a new temporary directory under the output parent. On failure it removes its incomplete output and staging directory.

The package contains exactly these three files:

- `graphhelm-windows-x86_64.zip`: the only archive entry is `bin/graphhelm.exe`.
- `manifest.json`: format version, target `x86_64-pc-windows-msvc`, declared source SHA, binary SHA-256 and byte size, and archive SHA-256 and byte size.
- `SHA256SUMS`: SHA-256 for the ZIP file.

The declared source SHA is caller-supplied attribution. It does not prove which source produced the executable. A synthetic PE fixture can verify packaging behavior, but cannot show that a real GraphHelm executable works. This local package is not a release upload or an update feed.
