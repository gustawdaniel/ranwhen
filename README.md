# ranwhen – Visualize when your system was running

ranwhen graphically shows **in your terminal** when your system was running in the past. Have a look:

![Screenshot](Screenshot.png?raw=true)


# Performance & Rust Rewrite

`ranwhen` was originally written in Python, but has been completely rewritten in **Rust** for maximum performance and zero startup latency while preserving 100% byte-for-byte output compatibility.

### Benchmarks

**Environment:** Arch Linux x86_64, Linux 7.2.3, AMD Ryzen  
**Dataset:** Real system `/var/log/wtmp` covering **679 days** of uptime history (32,592 time slots, 563 reboot entries).  
**Methodology:** Measured using [`hyperfine`](https://github.com/sharkdp/hyperfine) with 5 warmup runs across 50 benchmark runs.

```bash
hyperfine --warmup 5 --runs 50 './ranwhen.py' './target/release/ranwhen'
```

| Implementation | Mean ± σ | Min | Max | Relative Speedup |
| :--- | :---: | :---: | :---: | :---: |
| **Rust (`ranwhen`)** | **6.6 ms ± 0.3 ms** | **6.1 ms** | **7.5 ms** | **1.00** *(baseline)* |
| **Python (`ranwhen.py`)** | **101.7 ms ± 3.5 ms** | **96.8 ms** | **111.5 ms** | **15.37 ± 0.86× slower** |

#### Interactive Terminal Execution (Fish Shell with ANSI rendering)
When rendering in an interactive terminal (including PTY allocation and terminal emulator escape sequence processing):
* **Rust**: **8.2 ms ± 1.7 ms** (cold peak: 28.5 ms)
* **Python**: **147.5 ms ± 36.1 ms** (cold peak: 207.3 ms)

#### Resource Usage & Verification
* **Memory Footprint (Peak RSS)**: **11.6 MB** (Rust) vs **25.4 MB** (Python).
* **Output Parity**: **100% byte-for-byte identical** verified with `diff` and `md5sum` (`0134ab2f6c031b95465007f04db4ae5e`).

### Key Improvements & Bugfixes:
1. **Zero Startup Overhead**: Native machine code eliminates Python runtime initialization, dynamic module loading, regex compilation, and object allocation bottlenecks.
2. **Fixed Crash & Active Sessions**: Restored 196 missing reboot sessions (`crash` durations and `still running` active sessions) that were silently discarded by the original script, recovering hundreds of days of uptime data.
3. **Flexible Data Sources**: Seamlessly parses live `/var/log/wtmp`, rotated logs (`wtmp.1`), static text backups (`last` output), or piped standard input (`stdin`).


# Usage

### Native Rust Binary (Recommended)
Build and run the compiled Rust version:
```bash
cargo build --release
./target/release/ranwhen
```

### macOS Support & Remote Queries
macOS does not maintain a continuous Linux-style `/var/log/wtmp` reboot log across sleep cycles. `ranwhen` natively supports macOS by querying display backlit activity (CoreDuet) and power events (`pmset`), merging short gaps (< 2 minutes), and archiving them permanently into `~/.local/share/ranwhen/activity_sessions.log`.

* **Query a remote macOS machine over SSH**:
  ```bash
  ranwhen mac
  ranwhen hg
  # or explicitly:
  ranwhen --host user@macbook
  ```
* **Install the persistent background collector (LaunchAgent)** on macOS:
  ```bash
  ranwhen --install-daemon
  # or remotely from Linux:
  ranwhen --install-daemon --host mac
  ```
* **Check or uninstall daemon**:
  ```bash
  ranwhen --status-daemon [--host mac]
  ranwhen --uninstall-daemon [--host mac]
  ```

### Installation

#### Arch Linux (AUR)
Install using your preferred AUR helper (both package names `ranwhen` and `runwhen` are supported and install the native Rust binary with symlinks):
```bash
paru -S ranwhen
# or:
paru -S runwhen
# or development git version:
paru -S ranwhen-git
```
The PKGBUILD definitions are maintained in [`aur/PKGBUILD`](aur/PKGBUILD), [`aur/runwhen/PKGBUILD`](aur/runwhen/PKGBUILD), and [`aur/ranwhen-git/PKGBUILD`](aur/ranwhen-git/PKGBUILD).

#### macOS (Homebrew Tap)
Add the official tap and install `ranwhen`:
```bash
brew tap gustawdaniel/ranwhen
brew install ranwhen
```
The formula automatically registers the background LaunchAgent collector to ensure your activity history is preserved.
To install from source or HEAD:
```bash
brew install --HEAD gustawdaniel/ranwhen/ranwhen
```

### Python Version (Legacy)
```bash
./ranwhen.py
```

`ranwhen` automatically inspects `/var/log/wtmp` and any rotated uncompressed logs (`/var/log/wtmp.1`, etc.).

You can also pass:
- **A saved text output from `last`**:
  ```bash
  ranwhen last_backup.txt
  ```
- **A specific binary wtmp file**:
  ```bash
  ranwhen -f /var/log/wtmp
  # or
  ranwhen /path/to/wtmp
  ```
- **Standard input (piped)**:
  ```bash
  last -F -w -x | ranwhen -
  ```


# Requirements

* *nix system with [last(1)](http://linux.die.net/man/1/last) installed and supporting the -R and -F flags
* Rust (for building native binary) or [Python >= 3.2](http://www.python.org/)
* Terminal emulator with support for Unicode and xterm's 256 color mode


# License

Copyright © 2013 Philipp Emanuel Weidmann (<pew@worldwidemann.com>)
Rust rewrite © 2026 Daniel Gustaw (<gustaw.daniel@gmail.com>)

ranwhen is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.

ranwhen is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.

You should have received a copy of the GNU General Public License along with ranwhen.  If not, see <http://www.gnu.org/licenses/>.
