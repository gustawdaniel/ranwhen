// ranwhen – Visualize when your system was running
//
// Requirements:
// - *nix system with last(1) installed and supporting the -R and -F flags
// - Terminal emulator with support for Unicode and xterm's 256 color mode
//
// Copyright © 2013 Philipp Emanuel Weidmann <pew@worldwidemann.com>
// Rust rewrite © 2026 Daniel Gustaw <gustaw.daniel@gmail.com>
//
// Nemo vir est qui mundum non reddat meliorem.
//
// ranwhen is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, Timelike};
use regex::Regex;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;

mod macos;

const FOREGROUND_COLOR: u8 = 251;
const HEADING_COLOR: u8 = 208;
const HEADING_LINE_COLOR: u8 = 246;
const TIME_COLOR: u8 = 231;
const TIME_TEXT_COLOR: u8 = 246;
const HISTOGRAM_COLOR: [u8; 4] = [57, 56, 126, 197];
const HISTOGRAM_COLOR_GRID: [u8; 4] = [99, 97, 169, 204];
const WEEKDAY_COLOR: u8 = 245;
const WEEKEND_COLOR: u8 = 231;
const BAR_COLOR: u8 = 28;
const BAR_WEEKEND_COLOR: u8 = 82;
const BAR_COLOR_GRID: u8 = 77;
const BAR_WEEKEND_COLOR_GRID: u8 = 156;
const NIGHT_COLOR: u8 = 51;
const SUNRISE_COLOR: u8 = 228;
const NOON_COLOR: u8 = 226;
const SUNSET_COLOR: u8 = 214;
const GRID_COLOR: u8 = 238;

const LEVEL_CHARACTERS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
const OUTPUT_WIDTH: usize = 61;

struct Styler {
    use_escape_sequences: bool,
}

impl Styler {
    fn new() -> Self {
        Self {
            use_escape_sequences: io::stdout().is_terminal(),
        }
    }

    fn get_reset_sequence(&self) -> &'static str {
        if self.use_escape_sequences {
            "\x1b[0m"
        } else {
            ""
        }
    }

    fn get_escape_sequence(&self, fgcolor: Option<u8>, bgcolor: Option<u8>, bold: bool) -> String {
        if !self.use_escape_sequences {
            return String::new();
        }
        let fg = fgcolor.map(|c| format!("38;5;{}", c)).unwrap_or_default();
        let bg = bgcolor.map(|c| format!(";48;5;{}", c)).unwrap_or_default();
        let b = if bold { ";1" } else { "" };
        format!("{}\x1b[{}{}{}m", self.get_reset_sequence(), fg, bg, b)
    }

    fn style(&self, text: &str, fgcolor: Option<u8>, bgcolor: Option<u8>, bold: bool) -> String {
        format!(
            "{}{}{}",
            self.get_escape_sequence(fgcolor, bgcolor, bold),
            text,
            self.get_escape_sequence(Some(FOREGROUND_COLOR), None, false)
        )
    }
}

struct DeltaFields {
    hours: i64,
    minutes: i64,
}

fn get_delta_fields(total_seconds: i64) -> DeltaFields {
    let rounded = total_seconds.max(0);
    let hours = rounded / 3600;
    let remainder = rounded % 3600;
    let minutes = remainder / 60;
    DeltaFields { hours, minutes }
}

fn format_delta(styler: &Styler, total_seconds: i64) -> String {
    let fields = get_delta_fields(total_seconds);
    format!(
        "{}{}{}{}",
        styler.style(&format!("{:4}", fields.hours), Some(TIME_COLOR), None, false),
        styler.style(" hours ", Some(TIME_TEXT_COLOR), None, false),
        styler.style(&format!("{:2}", fields.minutes), Some(TIME_COLOR), None, false),
        styler.style(" minutes", Some(TIME_TEXT_COLOR), None, false),
    )
}

fn format_delta_short(styler: &Styler, total_seconds: i64) -> String {
    let fields = get_delta_fields(total_seconds);
    if fields.hours == 0 && fields.minutes == 0 {
        return String::new();
    }
    if fields.hours == 0 {
        format!(
            "{}{}",
            styler.style("  :", Some(TIME_TEXT_COLOR), None, false),
            styler.style(&format!("{:02}", fields.minutes), Some(TIME_COLOR), None, false)
        )
    } else {
        format!(
            "{}{}{}",
            styler.style(&format!("{:2}", fields.hours), Some(TIME_COLOR), None, false),
            styler.style(":", Some(TIME_TEXT_COLOR), None, false),
            styler.style(&format!("{:02}", fields.minutes), Some(TIME_COLOR), None, false)
        )
    }
}

fn format_heading(styler: &Styler, heading: &str) -> String {
    let padded_heading = format!(" {} ", heading);
    let padded_len = padded_heading.chars().count();
    let heading_pos = OUTPUT_WIDTH.saturating_sub(padded_len) / 2;
    let right_pos = OUTPUT_WIDTH.saturating_sub(heading_pos + padded_len);

    let left = format!("┌{}", "─".repeat(heading_pos));
    let right = format!("{}┐", "─".repeat(right_pos));

    format!(
        "{}{}{}",
        styler.style(&left, Some(HEADING_LINE_COLOR), None, false),
        styler.style(&padded_heading, Some(HEADING_COLOR), None, false),
        styler.style(&right, Some(HEADING_LINE_COLOR), None, false),
    )
}

#[derive(Clone, Copy, Debug)]
struct TimeSpan {
    from: NaiveDateTime,
    to: NaiveDateTime,
}

fn time_overlap(s1: &TimeSpan, s2: &TimeSpan) -> Duration {
    let start = s1.from.max(s2.from);
    let end = s1.to.min(s2.to);
    if start >= end {
        Duration::zero()
    } else {
        end - start
    }
}

fn parse_line(
    line: &str,
    start_re: &Regex,
    end_re: &Regex,
    dur_re: &Regex,
    now: NaiveDateTime,
) -> Option<TimeSpan> {
    let caps = start_re.captures(line)?;
    let from_str = caps.get(1)?.as_str();
    let from_time = NaiveDateTime::parse_from_str(from_str, "%b %e %H:%M:%S %Y").ok()?;
    let rest = caps.get(2).map(|m| m.as_str()).unwrap_or("");

    let to_time = if rest.contains("still running") {
        now
    } else if let Some(em) = end_re.captures(rest) {
        let to_str = em.get(1)?.as_str();
        NaiveDateTime::parse_from_str(to_str, "%b %e %H:%M:%S %Y").ok()?
    } else if let Some(dm) = dur_re.captures(rest) {
        let days: i64 = dm.get(1).map(|m| m.as_str().parse().unwrap_or(0)).unwrap_or(0);
        let hours: i64 = dm.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        let mins: i64 = dm.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        if hours < 0 {
            from_time
        } else {
            let delta = Duration::days(days) + Duration::hours(hours) + Duration::minutes(mins);
            from_time + delta
        }
    } else {
        return None;
    };

    let to_time = to_time.max(from_time);
    Some(TimeSpan { from: from_time, to: to_time })
}

fn is_binary_file<P: AsRef<Path>>(path: P) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut buf = [0u8; 1024];
        if let Ok(n) = f.read(&mut buf) {
            return buf[..n].contains(&0);
        }
    }
    false
}

fn run_last_command(extra_args: &[&str]) -> Result<Vec<String>, String> {
    let mut cmd = Command::new("last");
    cmd.args(["-R", "-F", "reboot"]);
    cmd.args(extra_args);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute 'last': {}. Please ensure util-linux is installed.", e))?;

    if !output.status.success() {
        return Err(format!(
            "'last' command failed with exit code {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().map(|s| s.to_string()).collect())
}

struct CliOptions {
    file_arg: Option<String>,
    wtmp_arg: Option<String>,
    host: Option<String>,
    collect: bool,
    install_daemon: bool,
    status_daemon: bool,
    uninstall_daemon: bool,
    raw: bool,
}

fn print_help() {
    println!("usage: ranwhen [-h] [-f WTMP_FILE] [--host HOST] [--collect] [--install-daemon] [--status-daemon] [--raw] [target]");
    println!();
    println!("ranwhen – Visualize when your system was running");
    println!();
    println!("positional arguments:");
    println!("  target                Path to a log file, '-' for stdin, or remote SSH host (e.g. 'mac', 'hg')");
    println!();
    println!("options:");
    println!("  -h, --help            show this help message and exit");
    println!("  -f, --file-wtmp WTMP_FILE");
    println!("                        Explicit binary wtmp file to pass to 'last -f'");
    println!("  --host HOST           Remote host to inspect via SSH");
    println!("  --collect             Collect & merge latest activity into persistent history vault");
    println!("  --install-daemon      Install & load background collection daemon (macOS launchd)");
    println!("  --status-daemon       Check status of background collection daemon");
    println!("  --uninstall-daemon    Uninstall background collection daemon");
    println!("  --raw                 Output synthetic 'last -F -w -x' lines without rendering calendar");
}

fn parse_cli_args() -> CliOptions {
    let args: Vec<String> = std::env::args().collect();
    let mut opts = CliOptions {
        file_arg: None,
        wtmp_arg: None,
        host: None,
        collect: false,
        install_daemon: false,
        status_daemon: false,
        uninstall_daemon: false,
        raw: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-f" | "--file-wtmp" => {
                i += 1;
                if i < args.len() {
                    opts.wtmp_arg = Some(args[i].clone());
                } else {
                    eprintln!("Error: -f / --file-wtmp requires an argument");
                    std::process::exit(1);
                }
            }
            arg if arg.starts_with("-f") => {
                opts.wtmp_arg = Some(arg[2..].to_string());
            }
            "--host" => {
                i += 1;
                if i < args.len() {
                    opts.host = Some(args[i].clone());
                } else {
                    eprintln!("Error: --host requires an argument");
                    std::process::exit(1);
                }
            }
            "--collect" => {
                opts.collect = true;
            }
            "--install-daemon" => {
                opts.install_daemon = true;
            }
            "--status-daemon" => {
                opts.status_daemon = true;
            }
            "--uninstall-daemon" => {
                opts.uninstall_daemon = true;
            }
            "--raw" => {
                opts.raw = true;
            }
            arg if !arg.starts_with('-') => {
                if opts.file_arg.is_none() {
                    opts.file_arg = Some(arg.to_string());
                }
            }
            unknown => {
                eprintln!("Warning: unknown option '{}'", unknown);
            }
        }
        i += 1;
    }

    // If positional target is not an existing file or '-', treat as host
    if let Some(ref target) = opts.file_arg {
        if target != "-" && !Path::new(target).exists() && opts.host.is_none() {
            opts.host = Some(target.clone());
            opts.file_arg = None;
        }
    }

    opts
}

fn get_default_lines() -> Result<Vec<String>, String> {
    if cfg!(target_os = "macos") {
        let (spans, live) = macos::collect_all_sessions(None, 60);
        Ok(macos::format_ranwhen_lines(&spans, live))
    } else {
        let mut existing_wtmp = Vec::new();
        if Path::new("/var/log/wtmp").exists() {
            existing_wtmp.push("/var/log/wtmp".to_string());
        }
        let mut idx = 1;
        while Path::new(&format!("/var/log/wtmp.{}", idx)).exists() {
            existing_wtmp.push(format!("/var/log/wtmp.{}", idx));
            idx += 1;
        }

        if existing_wtmp.len() > 1 {
            let mut cmd_args = Vec::new();
            for f in &existing_wtmp {
                cmd_args.push("-f");
                cmd_args.push(f.as_str());
            }
            run_last_command(&cmd_args)
        } else if existing_wtmp.len() == 1 && existing_wtmp[0] != "/var/log/wtmp" {
            run_last_command(&["-f", &existing_wtmp[0]])
        } else {
            run_last_command(&[])
        }
    }
}

fn get_input_lines(opts: &CliOptions) -> Result<Vec<String>, String> {
    if let Some(ref h) = opts.host {
        let (spans, live) = macos::collect_all_sessions(Some(h.as_str()), 60);
        return Ok(macos::format_ranwhen_lines(&spans, live));
    }

    if let Some(ref wtmp_file) = opts.wtmp_arg {
        return run_last_command(&["-f", wtmp_file]);
    }

    if let Some(ref path) = opts.file_arg {
        if path == "-" {
            let stdin = io::stdin();
            return Ok(stdin.lock().lines().filter_map(|l| l.ok()).collect());
        }
        if is_binary_file(path) {
            return run_last_command(&["-f", path]);
        } else {
            let f = File::open(path).map_err(|e| format!("Error reading file '{}': {}", path, e))?;
            let reader = BufReader::new(f);
            return Ok(reader.lines().filter_map(|l| l.ok()).collect());
        }
    }

    get_default_lines()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let opts = parse_cli_args();

    if opts.install_daemon {
        if let Err(e) = macos::install_daemon(opts.host.as_deref()) {
            eprintln!("Error installing daemon: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    if opts.status_daemon {
        macos::status_daemon(opts.host.as_deref());
        return Ok(());
    }

    if opts.uninstall_daemon {
        if let Err(e) = macos::uninstall_daemon(opts.host.as_deref()) {
            eprintln!("Error uninstalling daemon: {}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    if opts.collect {
        let (spans, _) = macos::collect_all_sessions(opts.host.as_deref(), 60);
        let p = macos::get_history_file_path(opts.host.as_deref());
        println!(
            "Collected and merged {} activity sessions into {}",
            spans.len(),
            p.display()
        );
        return Ok(());
    }

    let styler = Styler::new();

    let lines = match get_input_lines(&opts) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    if opts.raw {
        for l in &lines {
            println!("{}", l);
        }
        return Ok(());
    }

    let start_re = Regex::new(
        r"^reboot\s+system\s+boot\s+(?:(?:\S+)\s+)?(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s*(.*)"
    )?;
    let end_re = Regex::new(
        r"^-\s+(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})"
    )?;
    let dur_re = Regex::new(r"\((?:(\d+)\+)?(-?\d+):(\d+)\)")?;

    let now = Local::now().naive_local();
    let mut time_spans: Vec<TimeSpan> = lines
        .iter()
        .filter_map(|l| parse_line(l, &start_re, &end_re, &dur_re, now))
        .collect();

    if time_spans.is_empty() {
        eprintln!("Error: No parsable reboot records found.");
        std::process::exit(1);
    }

    // Sort and merge overlapping spans
    time_spans.sort_by_key(|s| s.from);
    let mut merged: Vec<TimeSpan> = Vec::with_capacity(time_spans.len());
    for s in time_spans {
        if let Some(prev) = merged.last_mut() {
            if s.from <= prev.to {
                prev.to = prev.to.max(s.to);
            } else {
                merged.push(s);
            }
        } else {
            merged.push(s);
        }
    }

    let day = Duration::days(1);
    let half_hour = Duration::minutes(30);
    let half_hours_in_day = 48usize;

    let min_from = merged.iter().map(|s| s.from).min().unwrap();
    let max_to = merged.iter().map(|s| s.to).max().unwrap();

    let earliest_time = min_from.date().and_hms_opt(0, 0, 0).unwrap();
    let latest_time = max_to.date().and_hms_opt(0, 0, 0).unwrap() + day;

    // Fast sweep-line slot overlap computation
    let mut time_slots = Vec::new();
    let mut aggregated_time_slots = vec![Duration::zero(); half_hours_in_day];
    let mut total_time = Duration::zero();

    let n_spans = merged.len();
    let mut span_idx = 0;
    let mut current_time = earliest_time;

    while current_time < latest_time {
        let slot_end = current_time + half_hour;
        while span_idx < n_spans && merged[span_idx].to <= current_time {
            span_idx += 1;
        }

        let mut time_in_slot = Duration::zero();
        let mut i = span_idx;
        while i < n_spans && merged[i].from < slot_end {
            let slot = TimeSpan { from: current_time, to: slot_end };
            time_in_slot = time_in_slot + time_overlap(&slot, &merged[i]);
            i += 1;
        }

        let half_hour_index = (current_time.hour() * 2 + if current_time.minute() >= 30 { 1 } else { 0 }) as usize;
        aggregated_time_slots[half_hour_index] = aggregated_time_slots[half_hour_index] + time_in_slot;
        total_time = total_time + time_in_slot;

        time_slots.push((current_time, time_in_slot));
        current_time = slot_end;
    }

    // Group time slots by date
    let mut slots_by_date: HashMap<NaiveDate, Vec<(NaiveDateTime, Duration)>> = HashMap::new();
    for (time, tis) in time_slots {
        slots_by_date.entry(time.date()).or_default().push((time, tis));
    }

    let time_header = format!(
        "       0:00 {}      6:00 {}     12:00 {}     18:00 {}     24:00 {}",
        styler.style("☾", Some(NIGHT_COLOR), None, false),
        styler.style("☀", Some(SUNRISE_COLOR), None, false),
        styler.style("☀", Some(NOON_COLOR), None, false),
        styler.style("☀", Some(SUNSET_COLOR), None, false),
        styler.style("☽", Some(NIGHT_COLOR), None, false)
    );

    let grid_header = styler.style("▆           ▆           ▆           ▆           ▆", Some(GRID_COLOR), None, false);
    let grid_footer = styler.style("▀           ▀           ▀           ▀           ▀", Some(GRID_COLOR), None, false);

    let mut writer = BufWriter::new(io::stdout().lock());

    // Print default foreground color
    write!(writer, "{}", styler.get_escape_sequence(Some(FOREGROUND_COLOR), None, false))?;

    // Print month views (chronological forward: oldest to newest)
    let mut cur = earliest_time;
    let mut current_month = 0u32;
    let levels = LEVEL_CHARACTERS.len() - 2; // 7

    while cur < latest_time {
        let month_changed = cur.month() != current_month;

        if month_changed {
            current_month = cur.month();
            writeln!(writer)?;
            writeln!(writer)?;
            writeln!(writer, "{}", format_heading(&styler, &cur.format("%B %Y").to_string()))?;
            writeln!(writer)?;
            writeln!(writer, "{}", time_header)?;
            writeln!(writer, "        {}", grid_header)?;
        }

        let weekday = cur.weekday().num_days_from_monday();
        let is_weekend = weekday == 5 || weekday == 6;
        let is_sunday = weekday == 6;

        let time_fg = if is_weekend { WEEKEND_COLOR } else { WEEKDAY_COLOR };
        let time_str = format!("{} {:2}", cur.format("%a"), cur.day());
        let mut output_line = format!("{}  ", styler.style(&time_str, Some(time_fg), None, is_sunday));

        let mut time_sum = Duration::zero();
        let mut bar_text = String::new();

        if let Some(day_slots) = slots_by_date.get(&cur.date()) {
            for (slot_index, (_t, tis)) in day_slots.iter().enumerate() {
                time_sum = time_sum + *tis;
                let ratio = tis.num_milliseconds() as f64 / half_hour.num_milliseconds() as f64;
                let level = (ratio * levels as f64).round() as usize;
                let level = level.min(levels);
                let grid = slot_index % 12 == 0;

                let fg = if is_weekend {
                    if grid { BAR_WEEKEND_COLOR_GRID } else { BAR_WEEKEND_COLOR }
                } else {
                    if grid { BAR_COLOR_GRID } else { BAR_COLOR }
                };
                let bg = if grid { Some(GRID_COLOR) } else { None };
                bar_text.push_str(&styler.style(LEVEL_CHARACTERS[level], Some(fg), bg, false));
            }
        }

        output_line.push_str(&bar_text);
        output_line.push_str(&styler.style(" ", None, Some(GRID_COLOR), false));
        output_line.push_str(" ");
        output_line.push_str(&format_delta_short(&styler, time_sum.num_seconds()));

        writeln!(writer, "{}", output_line)?;

        let next_day = cur + day;
        if next_day.month() != cur.month() || next_day >= latest_time {
            writeln!(writer, "        {}", grid_footer)?;
        }

        cur = next_day;
    }

    // Print summary at bottom
    let number_of_days = (latest_time.date() - earliest_time.date()).num_days().max(1);

    writeln!(writer)?;
    writeln!(writer)?;
    writeln!(
        writer,
        "{}{} – {} ({}{})",
        styler.style("Period:  ", None, None, true),
        earliest_time.format("%B %d %Y"),
        (latest_time - day).format("%B %d %Y"),
        styler.style(&format!("{}", number_of_days), Some(TIME_COLOR), None, false),
        styler.style(" days", Some(TIME_TEXT_COLOR), None, false),
    )?;
    writeln!(writer)?;
    writeln!(
        writer,
        "{}{}",
        styler.style("Total time running: ", None, None, true),
        format_delta(&styler, total_time.num_seconds())
    )?;
    let daily_avg_secs = total_time.num_seconds() / number_of_days;
    writeln!(
        writer,
        "{}{}",
        styler.style("Daily average:      ", None, None, true),
        format_delta(&styler, daily_avg_secs)
    )?;
    writeln!(writer)?;
    writeln!(writer)?;

    // Print histogram at bottom
    writeln!(writer, "{}", styler.style("Histogram:", None, None, true))?;
    writeln!(writer)?;
    writeln!(writer, "{}", time_header)?;
    writeln!(writer, "   max  {}", grid_header)?;

    let number_of_lines = 4usize;
    let hist_levels = LEVEL_CHARACTERS.len() - 1; // 8

    let min_secs = aggregated_time_slots.iter().map(|d| d.num_seconds()).min().unwrap_or(0) as f64 / number_of_days as f64;
    let max_secs = aggregated_time_slots.iter().map(|d| d.num_seconds()).max().unwrap_or(0) as f64 / number_of_days as f64;
    let half_hour_secs = half_hour.num_seconds() as f64;

    let min_level = min_secs / half_hour_secs;
    let max_level = max_secs / half_hour_secs;
    let level_range = max_level - min_level;

    for line_idx in (0..number_of_lines).rev() {
        let label = if line_idx == 0 { "   min" } else { "      " };
        let mut line_str = format!("{}  ", label);

        for (slot_index, ts) in aggregated_time_slots.iter().enumerate() {
            let slot_secs = ts.num_seconds() as f64 / number_of_days as f64;
            let mut level = slot_secs / half_hour_secs;
            level = if level_range > 0.0 { (level - min_level) / level_range } else { 0.0 };
            let rounded = (level * (hist_levels * number_of_lines) as f64).round() as isize - (line_idx * hist_levels) as isize;
            let clamped = rounded.clamp(0, hist_levels as isize) as usize;

            let grid = slot_index % 12 == 0;
            let fg = if grid { HISTOGRAM_COLOR_GRID[line_idx] } else { HISTOGRAM_COLOR[line_idx] };
            let bg = if grid { Some(GRID_COLOR) } else { None };
            line_str.push_str(&styler.style(LEVEL_CHARACTERS[clamped], Some(fg), bg, false));
        }
        line_str.push_str(&styler.style(" ", None, Some(GRID_COLOR), false));
        writeln!(writer, "{}", line_str)?;
    }

    writeln!(writer, "        {}", grid_footer)?;
    writeln!(writer)?;

    // Reset text attributes
    write!(writer, "{}", styler.get_reset_sequence())?;
    writer.flush()?;

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        if let Some(io_err) = e.downcast_ref::<io::Error>() {
            if io_err.kind() == io::ErrorKind::BrokenPipe {
                std::process::exit(0);
            }
        }
        eprintln!("{}", e);
        std::process::exit(1);
    }
}
