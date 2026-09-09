#!/usr/bin/env python3
"""
ranwhen macOS Proof of Concept & Persistent Daemon
===================================================
Extracts real system wake/sleep/display activity from macOS CoreDuet (`knowledgeC.db`)
and `pmset -g log`, synthesizes standard `last -F -w -x` reboot sessions, and provides
a persistent background daemon (launchd) so activity history is preserved indefinitely
(preventing Apple's 14-30 day log rotation from purging old records).

Usage:
  # View activity from Linux (over SSH):
  ./ranwhen_macos.py mac
  ./ranwhen_macos.py hg

  # View activity locally on macOS:
  ./ranwhen_macos.py

  # Collect & backup latest history into ~/.local/share/ranwhen/activity_sessions.log:
  ./ranwhen_macos.py --collect
  ./ranwhen_macos.py mac --collect

  # Install background daemon on macOS (runs every 30 min via launchd):
  ./ranwhen_macos.py --install-daemon
  ./ranwhen_macos.py mac --install-daemon

  # Daemon status:
  ./ranwhen_macos.py --status-daemon
  ./ranwhen_macos.py mac --status-daemon

  # Raw synthetic 'last' lines:
  ./ranwhen_macos.py mac --raw
"""

import sys
import os
import re
import argparse
import subprocess
import shutil
from datetime import datetime
from pathlib import Path

DATE_REGEX = re.compile(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) [+-]\d{4}\s+(\S+)\s*(.*)")
PLIST_LABEL = "com.gustawdaniel.ranwhen-collector"

def get_history_file_path(host=None):
    base_dir = Path.home() / ".local" / "share" / "ranwhen"
    base_dir.mkdir(parents=True, exist_ok=True)
    if host:
        target_file = base_dir / f"activity_sessions_{host}.log"
        old_file = base_dir / f"history_{host}.txt"
    else:
        target_file = base_dir / "activity_sessions.log"
        old_file = base_dir / "history.txt"

    if not target_file.exists() and old_file.exists():
        try:
            old_file.rename(target_file)
        except OSError:
            pass
    return target_file

def extract_from_knowledge(host=None, min_duration_sec=60):
    """
    Extracts active display backlight sessions from macOS CoreDuet knowledgeC.db (up to 30 days).
    """
    query = (
        "SELECT datetime(ZSTARTDATE + 978307200, 'unixepoch', 'localtime'), "
        "       datetime(ZENDDATE + 978307200, 'unixepoch', 'localtime') "
        "FROM ZOBJECT "
        f"WHERE ZSTREAMNAME = '/display/isBacklit' AND ZVALUEINTEGER = 1 AND (ZENDDATE - ZSTARTDATE) >= {min_duration_sec} "
        "ORDER BY ZSTARTDATE ASC;"
    )
    if host:
        cmd = ["ssh", host, f"sqlite3 ~/Library/Application\\ Support/Knowledge/knowledgeC.db \"{query}\""]
    else:
        db_path = os.path.expanduser("~/Library/Application Support/Knowledge/knowledgeC.db")
        if not os.path.exists(db_path):
            return []
        cmd = ["sqlite3", db_path, query]

    try:
        out = subprocess.check_output(cmd, text=True, stderr=subprocess.DEVNULL)
    except Exception:
        return []

    spans = []
    for row in out.strip().split("\n"):
        if not row or "|" not in row:
            continue
        parts = row.split("|")
        try:
            s_dt = datetime.strptime(parts[0], "%Y-%m-%d %H:%M:%S")
            e_dt = datetime.strptime(parts[1], "%Y-%m-%d %H:%M:%S")
            spans.append((s_dt, e_dt))
        except ValueError:
            continue
    return spans

def extract_events_from_pmset(host=None, mode="interactive"):
    """
    Parses `pmset -g log` into START / END events (covers current live boot and power transitions).
    """
    if host:
        cmd = ["ssh", host, "pmset -g log"]
    else:
        if sys.platform != "darwin":
            return []
        cmd = ["pmset", "-g", "log"]

    try:
        proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    except Exception:
        return []

    events = []
    for line in proc.stdout:
        m = DATE_REGEX.match(line)
        if not m:
            continue
        dt_str, category, rest = m.group(1), m.group(2), m.group(3)
        try:
            dt = datetime.strptime(dt_str, "%Y-%m-%d %H:%M:%S")
        except ValueError:
            continue

        if mode == "interactive":
            if "Display is turned on" in rest or (
                category == "Wake" and any(k in rest for k in ("due to UserActivity", "due to smc.sysState.Wake", "due to acattach", "HID Activity"))
            ):
                events.append((dt, "START"))
            elif "Display is turned off" in rest or (
                category == "Sleep" and any(k in rest for k in ("Clamshell Sleep", "Idle Sleep", "Software Sleep"))
            ):
                events.append((dt, "END"))
        else:
            if category == "Wake" and "Wake Requests" not in rest:
                events.append((dt, "START"))
            elif category == "Sleep":
                events.append((dt, "END"))

    proc.wait()
    return events

def pmset_events_to_spans(events, min_duration_sec=60):
    events.sort(key=lambda x: x[0])
    spans = []
    cur_start = None

    for dt, ev in events:
        if ev == "START":
            if cur_start is None:
                cur_start = dt
        elif ev == "END":
            if cur_start is not None:
                dur = (dt - cur_start).total_seconds()
                if dur >= min_duration_sec:
                    spans.append((cur_start, dt))
                cur_start = None

    if cur_start is not None:
        spans.append((cur_start, None))  # Still running

    return spans

def merge_spans(spans, max_gap_sec=60):
    """
    Sorts and merges overlapping or contiguous spans within `max_gap_sec`.
    """
    if not spans:
        return []
    # Filter out None starts if any
    valid_spans = [s for s in spans if s[0] is not None]
    valid_spans.sort(key=lambda s: s[0])

    merged = []
    for start, end in valid_spans:
        if not merged:
            merged.append((start, end))
            continue
        prev_start, prev_end = merged[-1]
        if prev_end is None:
            # Previous is still running
            continue
        if start <= prev_end:
            new_end = max(prev_end, end) if end else None
            merged[-1] = (prev_start, new_end)
        elif end is not None and (start - prev_end).total_seconds() <= max_gap_sec:
            merged[-1] = (prev_start, max(prev_end, end))
        else:
            merged.append((start, end))

    return merged

def parse_history_file(filepath):
    """
    Reads existing spans from history file in `last -F -w -x` format.
    """
    if not os.path.exists(filepath):
        return []
    spans = []
    # e.g.: reboot system boot Wed Aug 12 12:35:52 2026 - Wed Aug 12 13:00:32 2026 (00:24)
    line_re = re.compile(
        r"^reboot\s+system\s+boot\s+(?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+)?([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})"
        r"(?:\s+-\s+(?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+)?([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4}))?"
    )
    with open(filepath, "r", encoding="utf-8") as f:
        for line in f:
            m = line_re.match(line)
            if not m:
                continue
            s_str, e_str = m.group(1), m.group(2)
            try:
                s_dt = datetime.strptime(re.sub(r"\s+", " ", s_str), "%b %d %H:%M:%S %Y")
                e_dt = datetime.strptime(re.sub(r"\s+", " ", e_str), "%b %d %H:%M:%S %Y") if e_str else None
                spans.append((s_dt, e_dt))
            except ValueError:
                continue
    return spans

def save_history_file(filepath, spans):
    """
    Persists non-open spans to disk atomically.
    """
    lines = []
    for start, end in spans:
        if end is None:
            continue  # Don't freeze open sessions into permanent history file
        s_fmt = start.strftime("%a %b %e %H:%M:%S %Y")
        e_fmt = end.strftime("%a %b %e %H:%M:%S %Y")
        dur = end - start
        days = dur.days
        hours, remainder = divmod(dur.seconds, 3600)
        mins, _ = divmod(remainder, 60)
        dur_str = f"({days}+{hours:02d}:{mins:02d})" if days > 0 else f"({hours:02d}:{mins:02d})"
        lines.append(f"reboot   system boot  {s_fmt} - {e_fmt}  {dur_str}\n")

    tmp_path = f"{filepath}.tmp"
    with open(tmp_path, "w", encoding="utf-8") as f:
        f.writelines(lines)
    os.replace(tmp_path, filepath)

def format_ranwhen_lines(spans):
    lines = []
    for start, end in spans:
        s_fmt = start.strftime("%a %b %e %H:%M:%S %Y")
        if end is None:
            lines.append(f"reboot   system boot  {s_fmt}   still running")
        else:
            e_fmt = end.strftime("%a %b %e %H:%M:%S %Y")
            dur = end - start
            days = dur.days
            hours, remainder = divmod(dur.seconds, 3600)
            mins, _ = divmod(remainder, 60)
            dur_str = f"({days}+{hours:02d}:{mins:02d})" if days > 0 else f"({hours:02d}:{mins:02d})"
            lines.append(f"reboot   system boot  {s_fmt} - {e_fmt}  {dur_str}")
    return lines

def collect_all_sessions(host=None, min_duration_sec=60):
    """
    Combines existing history file + CoreDuet knowledge + pmset live events.
    """
    history_file = get_history_file_path(host=host)
    existing_spans = parse_history_file(history_file)

    # 1. Harvest from CoreDuet
    knowledge_spans = extract_from_knowledge(host=host, min_duration_sec=min_duration_sec)

    # 2. Harvest from pmset (for current live session)
    pm_events = extract_events_from_pmset(host=host)
    pm_spans = pmset_events_to_spans(pm_events, min_duration_sec=min_duration_sec)

    # Combine all closed spans
    all_closed = [s for s in (existing_spans + knowledge_spans + [s for s in pm_spans if s[1] is not None])]
    merged_closed = merge_spans(all_closed, max_gap_sec=120)

    # Save to persistent history file
    save_history_file(history_file, merged_closed)

    # Check if there is a live running session
    live_open = [s for s in pm_spans if s[1] is None]
    if live_open:
        last_start = live_open[-1][0]
        # If last_start is after the latest closed span, append as still running
        if not merged_closed or last_start >= merged_closed[-1][1]:
            merged_closed.append((last_start, None))

    return merged_closed

def install_launchd_daemon(host=None):
    """
    Installs and activates a macOS LaunchAgent running collection every 30 minutes.
    """
    plist_content = f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>/usr/bin/python3</string>
        <string>/Users/daniel/pro/ranwhen/ranwhen_macos.py</string>
        <string>--collect</string>
    </array>
    <key>StartInterval</key>
    <integer>1800</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/daniel/.local/share/ranwhen/collector.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/daniel/.local/share/ranwhen/collector.err</string>
</dict>
</plist>
"""
    plist_filename = f"{PLIST_LABEL}.plist"
    if host:
        # Deploy remotely
        tmp_plist = f"/tmp/{plist_filename}"
        with open(tmp_plist, "w") as f:
            f.write(plist_content)
        subprocess.run(["scp", tmp_plist, f"{host}:~/Library/LaunchAgents/{plist_filename}"], check=True)
        subprocess.run(["ssh", host, f"launchctl unload ~/Library/LaunchAgents/{plist_filename} 2>/dev/null || true"], check=False)
        subprocess.run(["ssh", host, f"launchctl load -w ~/Library/LaunchAgents/{plist_filename}"], check=True)
        os.remove(tmp_plist)
        print(f"Successfully installed and loaded launchd daemon on {host}!")
    else:
        agent_dir = Path.home() / "Library" / "LaunchAgents"
        agent_dir.mkdir(parents=True, exist_ok=True)
        plist_path = agent_dir / plist_filename
        with open(plist_path, "w") as f:
            f.write(plist_content)
        subprocess.run(["launchctl", "unload", str(plist_path)], stderr=subprocess.DEVNULL, check=False)
        subprocess.run(["launchctl", "load", "-w", str(plist_path)], check=True)
        print("Successfully installed and loaded launchd daemon locally!")

def check_daemon_status(host=None):
    cmd = ["ssh", host, f"launchctl list | grep {PLIST_LABEL} || true"] if host else ["launchctl", "list"]
    out = subprocess.check_output(cmd, text=True)
    if PLIST_LABEL in out:
        print(f"Daemon '{PLIST_LABEL}' is ACTIVE on {host or 'localhost'}:")
        for line in out.splitlines():
            if PLIST_LABEL in line:
                print(f"  {line}")
    else:
        print(f"Daemon '{PLIST_LABEL}' is NOT currently loaded on {host or 'localhost'}.")

def find_ranwhen_binary():
    script_dir = os.path.dirname(os.path.abspath(__file__))
    candidates = [
        os.path.join(script_dir, "target", "release", "ranwhen"),
        os.path.join(script_dir, "target", "debug", "ranwhen"),
        shutil.which("ranwhen"),
        os.path.expanduser("~/ranwhen"),
        os.path.expanduser("~/pro/ranwhen/target/release/ranwhen"),
        os.path.join(script_dir, "ranwhen.py"),
    ]
    for c in candidates:
        if c and os.path.isfile(c) and os.access(c, os.X_OK):
            return c
    return None

def main():
    parser = argparse.ArgumentParser(
        description="ranwhen macOS Proof of Concept & Persistent Daemon"
    )
    parser.add_argument(
        "host",
        nargs="?",
        default=None,
        help="SSH host name (e.g. 'mac', 'hg'). If omitted, operates locally on macOS.",
    )
    parser.add_argument(
        "--collect",
        action="store_true",
        help="Harvest latest activity and append/merge into permanent history file (~/.local/share/ranwhen/activity_sessions.log)",
    )
    parser.add_argument(
        "--install-daemon",
        action="store_true",
        help="Install macOS launchd background daemon (runs collection every 30m)",
    )
    parser.add_argument(
        "--status-daemon",
        action="store_true",
        help="Check status of background launchd daemon",
    )
    parser.add_argument(
        "--min-duration",
        type=int,
        default=60,
        help="Minimum session duration in seconds to record/display (default: 60s)",
    )
    parser.add_argument(
        "--raw",
        action="store_true",
        help="Output raw synthetic 'last -F -w -x' text lines without calling ranwhen renderer",
    )
    parser.add_argument(
        "--binary",
        default=None,
        help="Explicit path to ranwhen renderer binary or script",
    )

    args = parser.parse_args()

    if args.install_daemon:
        install_launchd_daemon(host=args.host)
        return

    if args.status_daemon:
        check_daemon_status(host=args.host)
        return

    # Collect sessions
    spans = collect_all_sessions(host=args.host, min_duration_sec=args.min_duration)

    if args.collect:
        history_path = get_history_file_path(host=args.host)
        print(f"Collected and merged {len(spans)} activity sessions into {history_path}")
        return

    lines = format_ranwhen_lines(spans)
    if not lines:
        print("No active sessions found.", file=sys.stderr)
        sys.exit(0)

    if args.raw:
        for l in lines:
            print(l)
        return

    renderer = args.binary or find_ranwhen_binary()
    if not renderer:
        print("Warning: ranwhen binary not found, displaying synthetic 'last' lines:", file=sys.stderr)
        for l in lines:
            print(l)
        return

    payload = "\n".join(lines) + "\n"
    render_cmd = [renderer, "-"]
    try:
        subprocess.run(render_cmd, input=payload, text=True, check=True)
    except Exception as e:
        print(f"Error executing renderer {renderer}: {e}", file=sys.stderr)
        for l in lines:
            print(l)

if __name__ == "__main__":
    main()
