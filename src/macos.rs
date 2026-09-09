// ranwhen – macOS activity collection and persistent launchd daemon
//
// Extracts actual screen / power sessions from CoreDuet (knowledgeC.db) and pmset,
// stores them persistently into ~/.local/share/ranwhen/activity_sessions.log, and manages
// launchd background service.

use chrono::{Duration, NaiveDateTime};
use regex::Regex;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PLIST_LABEL: &str = "com.gustawdaniel.ranwhen-collector";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub from: NaiveDateTime,
    pub to: NaiveDateTime,
}

pub fn get_history_file_path(host: Option<&str>) -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = PathBuf::from(home).join(".local").join("share").join("ranwhen");
    let _ = fs::create_dir_all(&dir);

    let target_file = if let Some(h) = host {
        dir.join(format!("activity_sessions_{}.log", h))
    } else {
        dir.join("activity_sessions.log")
    };

    // Automatic migration from old history.txt / history_<host>.txt if present
    if !target_file.exists() {
        let old_file = if let Some(h) = host {
            dir.join(format!("history_{}.txt", h))
        } else {
            dir.join("history.txt")
        };
        if old_file.exists() {
            let _ = fs::rename(&old_file, &target_file);
        }
    }

    target_file
}

pub fn extract_from_knowledge(host: Option<&str>, min_duration_sec: i64) -> Vec<Span> {
    let query = format!(
        "SELECT datetime(ZSTARTDATE + 978307200, 'unixepoch', 'localtime'), \
                datetime(ZENDDATE + 978307200, 'unixepoch', 'localtime') \
         FROM ZOBJECT \
         WHERE ZSTREAMNAME = '/display/isBacklit' AND ZVALUEINTEGER = 1 AND (ZENDDATE - ZSTARTDATE) >= {} \
         ORDER BY ZSTARTDATE ASC;",
        min_duration_sec
    );

    let output = if let Some(h) = host {
        let ssh_cmd = format!(
            "sqlite3 ~/Library/Application\\ Support/Knowledge/knowledgeC.db \"{}\"",
            query
        );
        Command::new("ssh").args([h, &ssh_cmd]).output()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "".to_string());
        let db_path = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Knowledge")
            .join("knowledgeC.db");
        if !db_path.exists() {
            return Vec::new();
        }
        Command::new("/usr/bin/sqlite3")
            .arg(db_path)
            .arg(&query)
            .output()
    };

    let Ok(out) = output else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let mut spans = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 2 {
            continue;
        }
        let Ok(s) = NaiveDateTime::parse_from_str(parts[0], "%Y-%m-%d %H:%M:%S") else {
            continue;
        };
        let Ok(e) = NaiveDateTime::parse_from_str(parts[1], "%Y-%m-%d %H:%M:%S") else {
            continue;
        };
        if e >= s {
            spans.push(Span { from: s, to: e });
        }
    }
    spans
}

pub fn extract_from_pmset(host: Option<&str>, min_duration_sec: i64) -> (Vec<Span>, Option<NaiveDateTime>) {
    let output = if let Some(h) = host {
        Command::new("ssh").args([h, "pmset -g log"]).output()
    } else {
        if cfg!(not(target_os = "macos")) && host.is_none() {
            return (Vec::new(), None);
        }
        Command::new("pmset").args(["-g", "log"]).output()
    };

    let Ok(out) = output else { return (Vec::new(), None) };
    if !out.status.success() {
        return (Vec::new(), None);
    }

    let text = String::from_utf8_lossy(&out.stdout);
    let date_re = match Regex::new(r"^(\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}) [+-]\d{4}\s+(\S+)\s*(.*)") {
        Ok(r) => r,
        Err(_) => return (Vec::new(), None),
    };

    let mut events: Vec<(NaiveDateTime, bool)> = Vec::new(); // (dt, is_start)

    for line in text.lines() {
        let Some(caps) = date_re.captures(line) else { continue };
        let dt_str = caps.get(1).map_or("", |m| m.as_str());
        let category = caps.get(2).map_or("", |m| m.as_str());
        let rest = caps.get(3).map_or("", |m| m.as_str());

        let Ok(dt) = NaiveDateTime::parse_from_str(dt_str, "%Y-%m-%d %H:%M:%S") else { continue };

        if rest.contains("Display is turned on")
            || (category == "Wake"
                && (rest.contains("due to UserActivity")
                    || rest.contains("due to smc.sysState.Wake")
                    || rest.contains("due to acattach")
                    || rest.contains("HID Activity")))
        {
            events.push((dt, true));
        } else if rest.contains("Display is turned off")
            || (category == "Sleep"
                && (rest.contains("Clamshell Sleep")
                    || rest.contains("Idle Sleep")
                    || rest.contains("Software Sleep")))
        {
            events.push((dt, false));
        }
    }

    events.sort_by_key(|e| e.0);

    let mut spans = Vec::new();
    let mut cur_start: Option<NaiveDateTime> = None;

    for (dt, is_start) in events {
        if is_start {
            if cur_start.is_none() {
                cur_start = Some(dt);
            }
        } else if let Some(st) = cur_start {
            let dur = dt - st;
            if dur >= Duration::seconds(min_duration_sec) {
                spans.push(Span { from: st, to: dt });
            }
            cur_start = None;
        }
    }

    (spans, cur_start)
}

pub fn merge_spans(mut spans: Vec<Span>, max_gap_sec: i64) -> Vec<Span> {
    if spans.is_empty() {
        return spans;
    }
    spans.sort_by_key(|s| s.from);
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());

    for s in spans {
        if let Some(prev) = merged.last_mut() {
            if s.from <= prev.to {
                prev.to = prev.to.max(s.to);
            } else if (s.from - prev.to).num_seconds() <= max_gap_sec {
                prev.to = prev.to.max(s.to);
            } else {
                merged.push(s);
            }
        } else {
            merged.push(s);
        }
    }
    merged
}

pub fn parse_history_file(path: &Path) -> Vec<Span> {
    let Ok(f) = File::open(path) else { return Vec::new() };
    let reader = BufReader::new(f);

    let line_re = match Regex::new(
        r"^reboot\s+system\s+boot\s+(?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+)?([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})(?:\s+-\s+(?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+)?([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4}))?"
    ) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut spans = Vec::new();
    for line in reader.lines().flatten() {
        let Some(caps) = line_re.captures(&line) else { continue };
        let s_str = caps.get(1).map_or("", |m| m.as_str());
        let e_str = caps.get(2).map(|m| m.as_str());

        let Ok(s) = NaiveDateTime::parse_from_str(s_str, "%b %e %H:%M:%S %Y") else { continue };
        if let Some(estr) = e_str {
            if let Ok(e) = NaiveDateTime::parse_from_str(estr, "%b %e %H:%M:%S %Y") {
                if e >= s {
                    spans.push(Span { from: s, to: e });
                }
            }
        }
    }
    spans
}

pub fn save_history_file(path: &Path, spans: &[Span]) -> io::Result<()> {
    let tmp_path = path.with_extension("tmp");
    {
        let mut out = File::create(&tmp_path)?;
        for s in spans {
            let s_fmt = s.from.format("%a %b %e %H:%M:%S %Y");
            let e_fmt = s.to.format("%a %b %e %H:%M:%S %Y");
            let dur = s.to - s.from;
            let days = dur.num_days();
            let hours = dur.num_hours() % 24;
            let mins = dur.num_minutes() % 60;
            let dur_str = if days > 0 {
                format!("({}+{:02}:{:02})", days, hours, mins)
            } else {
                format!("({:02}:{:02})", hours, mins)
            };
            writeln!(out, "reboot   system boot  {} - {}  {}", s_fmt, e_fmt, dur_str)?;
        }
        out.flush()?;
    }
    fs::rename(tmp_path, path)
}

pub fn collect_all_sessions(host: Option<&str>, min_duration_sec: i64) -> (Vec<Span>, Option<NaiveDateTime>) {
    let history_file = get_history_file_path(host);
    let mut all_spans = parse_history_file(&history_file);

    // 1. CoreDuet knowledge spans
    let k_spans = extract_from_knowledge(host, min_duration_sec);
    all_spans.extend(k_spans);

    // 2. pmset spans
    let (pm_spans, live_open) = extract_from_pmset(host, min_duration_sec);
    all_spans.extend(pm_spans);

    // Merge all closed spans
    let merged = merge_spans(all_spans, 120);

    // Persist to activity_sessions.log
    let _ = save_history_file(&history_file, &merged);

    // Check if live session is currently open
    let live_session = if let Some(last_wake) = live_open {
        if merged.is_empty() || last_wake >= merged.last().unwrap().to {
            Some(last_wake)
        } else {
            None
        }
    } else {
        None
    };

    (merged, live_session)
}

pub fn format_ranwhen_lines(spans: &[Span], live_session: Option<NaiveDateTime>) -> Vec<String> {
    let mut lines = Vec::with_capacity(spans.len() + 1);
    for s in spans {
        let s_fmt = s.from.format("%a %b %e %H:%M:%S %Y");
        let e_fmt = s.to.format("%a %b %e %H:%M:%S %Y");
        let dur = s.to - s.from;
        let days = dur.num_days();
        let hours = dur.num_hours() % 24;
        let mins = dur.num_minutes() % 60;
        let dur_str = if days > 0 {
            format!("({}+{:02}:{:02})", days, hours, mins)
        } else {
            format!("({:02}:{:02})", hours, mins)
        };
        lines.push(format!("reboot   system boot  {} - {}  {}", s_fmt, e_fmt, dur_str));
    }
    if let Some(live) = live_session {
        let s_fmt = live.format("%a %b %e %H:%M:%S %Y");
        lines.push(format!("reboot   system boot  {}   still running", s_fmt));
    }
    lines
}

pub fn install_daemon(host: Option<&str>) -> Result<(), String> {
    let binary_path = if host.is_some() {
        "/Users/daniel/.local/bin/ranwhen".to_string()
    } else {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/usr/local/bin/ranwhen".to_string())
    };

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
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
"#,
        PLIST_LABEL, binary_path
    );

    let plist_name = format!("{}.plist", PLIST_LABEL);

    if let Some(h) = host {
        let tmp_path = format!("/tmp/{}", plist_name);
        fs::write(&tmp_path, &plist_content).map_err(|e| e.to_string())?;
        let _ = Command::new("scp").args([&tmp_path, &format!("{}:~/Library/LaunchAgents/{}", h, plist_name)]).status();
        let _ = Command::new("ssh").args([h, &format!("launchctl unload ~/Library/LaunchAgents/{} 2>/dev/null || true", plist_name)]).status();
        let status = Command::new("ssh").args([h, &format!("launchctl load -w ~/Library/LaunchAgents/{}", plist_name)]).status();
        let _ = fs::remove_file(tmp_path);
        if status.map_or(false, |s| s.success()) {
            println!("Successfully installed and loaded launchd daemon on {}!", h);
            Ok(())
        } else {
            Err(format!("Failed to load launchd daemon on {}", h))
        }
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let agent_dir = PathBuf::from(home).join("Library").join("LaunchAgents");
        fs::create_dir_all(&agent_dir).map_err(|e| e.to_string())?;
        let plist_path = agent_dir.join(&plist_name);
        fs::write(&plist_path, &plist_content).map_err(|e| e.to_string())?;

        let _ = Command::new("launchctl").args(["unload", plist_path.to_str().unwrap()]).status();
        let status = Command::new("launchctl").args(["load", "-w", plist_path.to_str().unwrap()]).status();

        if status.map_or(false, |s| s.success()) {
            println!("Successfully installed and loaded launchd daemon locally!");
            Ok(())
        } else {
            Err("Failed to load launchd daemon".to_string())
        }
    }
}

pub fn status_daemon(host: Option<&str>) {
    let out = if let Some(h) = host {
        Command::new("ssh")
            .args([h, &format!("launchctl list | grep {} || true", PLIST_LABEL)])
            .output()
    } else {
        Command::new("launchctl").arg("list").output()
    };

    let Ok(output) = out else {
        println!("Error querying launchctl.");
        return;
    };

    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains(PLIST_LABEL) {
        println!("Daemon '{}' is ACTIVE on {}:", PLIST_LABEL, host.unwrap_or("localhost"));
        for line in text.lines().filter(|l| l.contains(PLIST_LABEL)) {
            println!("  {}", line);
        }
    } else {
        println!("Daemon '{}' is NOT currently loaded on {}.", PLIST_LABEL, host.unwrap_or("localhost"));
    }
}

pub fn uninstall_daemon(host: Option<&str>) -> Result<(), String> {
    let plist_name = format!("{}.plist", PLIST_LABEL);
    if let Some(h) = host {
        let _ = Command::new("ssh").args([h, &format!("launchctl unload -w ~/Library/LaunchAgents/{} 2>/dev/null || true", plist_name)]).status();
        let _ = Command::new("ssh").args([h, &format!("rm -f ~/Library/LaunchAgents/{}", plist_name)]).status();
        println!("Uninstalled launchd daemon from {}.", h);
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let plist_path = PathBuf::from(home).join("Library").join("LaunchAgents").join(&plist_name);
        let _ = Command::new("launchctl").args(["unload", "-w", plist_path.to_str().unwrap()]).status();
        let _ = fs::remove_file(plist_path);
        println!("Uninstalled launchd daemon locally.");
    }
    Ok(())
}
