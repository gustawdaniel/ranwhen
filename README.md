# ranwhen – Visualize when your system was running

ranwhen graphically shows **in your terminal** when your system was running in the past. Have a look:

![Screenshot](Screenshot.png?raw=true)


# Performance & Rust Rewrite

`ranwhen` was originally written in Python, but has been completely rewritten in **Rust** for maximum performance and zero startup latency while preserving 100% byte-for-byte output compatibility.

### Real-World Benchmark (679 days of history, 32,592 time slots)

Measured on Arch Linux with Linux 6.11 / 7.2:

| Metric | Original / Python | Native Rust (`ranwhen`) | Improvement |
| :--- | :--- | :--- | :--- |
| **Execution Time (`fish` shell)** | **206.5 ms** | **12.8 ms – 28.5 ms** | **~16× faster** |
| **Raw Binary Runtime (`hyperfine`)** | **104.2 ms ± 3.4 ms** | **6.6 ms ± 0.3 ms** | **~15.7× faster** |
| **Memory Footprint (RSS)** | ~25 MB | ~2.5 MB | **10× less RAM** |
| **Output Verification** | Base | **100% identical** (identical MD5) | Exact parity |

### Key Improvements & Bugfixes:
1. **Blazing Fast**: Native machine code eliminates Python interpreter startup, regex compiling overhead, and datetime allocations.
2. **Fixed Crash & Active Sessions**: Restored 196 missing reboot sessions (`crash` durations and `still running` active sessions) that were silently discarded by the original script, recovering hundreds of days of uptime data.
3. **Flexible Data Sources**: Seamlessly parses live `/var/log/wtmp`, rotated logs (`wtmp.1`), static text backups (`last` output), or piped standard input (`stdin`).


# Usage

### Native Rust Binary (Recommended)
Build and run the compiled Rust version:
```bash
cargo build --release
./target/release/ranwhen
```

### Python Version
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
  last -F -w -x | ranwhen
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
