# gc-download

[![Crates.io][crates-badge]][crates-url]
[![License][license-badge]][license-url]

[crates-badge]: https://img.shields.io/crates/v/gc-download.svg
[crates-url]: https://crates.io/crates/gc-download
[license-badge]: https://img.shields.io/crates/l/gc-download
[license-url]: https://github.com/OpenKorabli/gc-download/blob/main/LICENSE

**Download and extract game files from the Wargaming Game Center (WGC),
Lesta Game Center (LGC), and WGC360 CN APIs.**

Project derived from [wgc-download](https://github.com/Monstrofil/wgc-download).

Supports World of Tanks, World of Warships, Tanks Blitz, World of Warplanes,
Mir Tankov, Mir Korabley, and CN region titles — listing available versions,
downloading `.dspkg` archive parts, and extracting individual files remotely
without downloading the full archive.

## Features

- **Multiple backend** — works with WGC (`-b wgc`), LGC (`-b lgc`, default),
  and CN360 WGC (`-b cn360`)
- **List games** — query the showroom for all available titles and regions
- **Inspect versions** — view patch parts, version ranges, and file listings
- **Download** — download full `.dspkg` files with progress bars and
  atomic writes
- **Remote extraction** — read the `.dspkg` (7z) archive header via HTTP
  range requests, then download and decompress only the files you need

## Usage

### List available games

```
$ gc-download games
Available games:

  MT.RU.PRODUCTION          Мир танков                Мир танков
  MK.RU.PRODUCTION          Мир кораблей              Мир кораблей
  WOTB.RU.PRODUCTION        Tanks Blitz
```

Specify a backend:

```
$ gc-download games -b wgc
Available games:

  WOT.EU.PRODUCTION         World of Tanks            Europe
  WOWS.WW.PRODUCTION        World of Warships         World of Warships
  WOTB.WW.PRODUCTION        WoT Blitz                 Worldwide
  WOWP.WW.PRODUCTION        World of Warplanes        Worldwide
```

Or WGC360 CN:

```
$ gc-download games -b cn360
Available games:

  WOT.CN.PRODUCTION         World of Tanks
  WOWS.CN.PRODUCTION        World of Warships
```

### Inspect a game

```
$ gc-download list MK.RU.PRODUCTION
Game:              MK.RU.PRODUCTION  (Мир кораблей — Мир кораблей)
Latest version:    26.6.1.0.8854215
Parts (4):
  sdcontent              1 file(s)      17.2 GB
  locale                 1 file(s)       5.3 MB
  client                 1 file(s)      33.1 GB
  hotfix                 1 file(s)      167.0 B
```

### List files in a part

```
$ gc-download list MK.RU.PRODUCTION --files locale
Part: locale
Files (1):
  mk_26.6.1.0.8854215_locale.dspkg              5.3 MB  (unpacked: 25.9 MB)
```

### Download a part

```
$ gc-download download MK.RU.PRODUCTION locale --all -d downloads/
```

### List archive contents remotely

```
$ gc-download extract MK.RU.PRODUCTION locale --list
Archive: mk_26.6.1.0.8854215_locale.dspkg (5.3 MB)
Reading archive index via range requests...
Index: 23 files (2 HTTP requests)
  bin/8854201/res/texts/ru/LC_MESSAGES/global.mo    7.0 MB  (compressed: 1.1 MB)
  ...
23 files total
```

### Extract a single file

```
$ gc-download extract MK.RU.PRODUCTION locale GameCheck/GameCheck_config.xml -d out/
```

### Extract with filter

```
$ gc-download extract MK.RU.PRODUCTION locale --filter '*.xml' -d out/
```

Specify a backend with `-b` (default is `lgc`). Available: `wgc`, `lgc`, `cn360`.

Select a CDN mirror with `-m` (WGC only):
- `asia` — `wguscs-wgcasia.wargaming.net`
- `na` — `wguscs-wgcna.wargaming.net`

```
$ gc-download -b wgc -m asia download WOT.ASIA.PRODUCTION locale --all
```

## Installation

```bash
cargo install gc-download
```

Or build from source:

```bash
git clone https://github.com/OpenKorabli/gc-download
cd gc-download
cargo install --path .
```

## How it works

1. **Showroom API** — fetches the game catalog (JSON) from the selected
   backend's showroom to resolve `app_id` → API base URL.
2. **Metadata + Patches Chain** — two XML endpoints provide version info,
   client types, part IDs, and CDN download URLs.
3. **Download** — files are fetched via HTTP GET with an `indicatif` progress
   bar and atomic writes.
4. **Remote extraction** — `sevenz-rust2` parses the 7z archive header through
   HTTP range requests. Only the compressed bytes of selected files are
   transferred over the wire, then decoded locally with Rust's native
   LZMA2/BCJ/Delta decompression.

## License

### Upstream (Monstrofil/wgc-download)

[MIT](https://github.com/Monstrofil/wgc-download/blob/master/LICENSE)

### This project

Copyright (C) 2026 OpenKorabli

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version.

This program is distributed in the hope that it will be useful, but WITHOUT ANY
WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A
PARTICULAR PURPOSE. See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with
this program. If not, see <https://www.gnu.org/licenses/>.
