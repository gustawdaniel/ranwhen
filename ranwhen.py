#!/usr/bin/env python3

# ranwhen – Visualize when your system was running
#
# Requirements:
# - *nix system with last(1) installed and supporting the -R and -F flags
# - Python >= 3.2
# - Terminal emulator with support for Unicode and xterm's 256 color mode
#
#
# Copyright © 2013 Philipp Emanuel Weidmann <pew@worldwidemann.com>
#
# Nemo vir est qui mundum non reddat meliorem.
#
#
# ranwhen is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# ranwhen is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with ranwhen.  If not, see <http://www.gnu.org/licenses/>.

import subprocess
import re
from datetime import datetime, timedelta
import sys
import os
import argparse
import signal

try:
    signal.signal(signal.SIGPIPE, signal.SIG_DFL)
except Exception:
    pass


# Regex patterns to parse last's output
# Supports:
# - Normal session: reboot system boot [...] Wed Jan 16 21:36:54 2013 - Wed Jan 16 22:05:50 2013 (00:28)
# - Active session: reboot system boot [...] Wed Sep  9 11:10:38 2026   still running
# - Crash session:  reboot system boot [...] Sat Aug 29 22:04:54 2026 - crash (00:19)
start_pattern = re.compile(
	r"^reboot\s+system\s+boot\s+(?:(?!(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\b)\S+\s+)?(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})\s*(.*)"
)
end_time_pattern = re.compile(
	r"^-\s+(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+([A-Za-z]{3}\s+[\s\d]\d\s+\d{2}:\d{2}:\d{2}\s+\d{4})"
)
duration_pattern = re.compile(
	r"\((?:(\d+)\+)?(-?\d+):(\d+)\)"
)

# Date format used by last
# e.g. "Jan 16 21:36:54 2013"
time_format = "%b %d %H:%M:%S %Y"

# Extracts start and end time from a line of last's output
def parse_line(line, now = None):
	if now is None:
		now = datetime.now().replace(microsecond = 0)
	result = start_pattern.match(line)
	if result is None:
		return None
	from_time_str = result.group(1)
	try:
		from_time = datetime.strptime(from_time_str, time_format)
	except ValueError:
		return None
	rest = result.group(2)

	if "still running" in rest:
		to_time = now
	else:
		end_result = end_time_pattern.match(rest)
		if end_result is not None:
			try:
				to_time = datetime.strptime(end_result.group(1), time_format)
			except ValueError:
				return None
		else:
			duration_result = duration_pattern.search(rest)
			if duration_result is not None:
				days = int(duration_result.group(1) or 0)
				hours = int(duration_result.group(2))
				mins = int(duration_result.group(3))
				delta = timedelta(days = days, hours = hours, minutes = mins) if hours >= 0 else timedelta()
				to_time = max(from_time, from_time + delta)
			else:
				return None

	if to_time < from_time:
		to_time = from_time

	return { "from" : from_time, "to" : to_time }


# Returns the length of time for which two time spans overlap
# (i.e. the length of their intersection)
def time_overlap(time_span_1, time_span_2):
	if max(time_span_1["from"], time_span_2["from"]) >= \
	   min(time_span_1["to"], time_span_2["to"]):
		# No overlap
		return timedelta()
	return min(time_span_1["to"], time_span_2["to"]) - \
	       max(time_span_1["from"], time_span_2["from"])


# Do not use escape sequences if output is piped
use_escape_sequences = sys.stdout.isatty()

# Returns an xterm escape sequence that, if printed to the terminal,
# will reset all character attributes to the default
def get_reset_sequence():
	if use_escape_sequences:
		return "\033[0m"
	return ""

# Returns an xterm escape sequence that, if printed to the terminal,
# will set the specified character attributes
def get_escape_sequence(fgcolor = None, bgcolor = None, bold = False):
	if use_escape_sequences:
		return get_reset_sequence() + \
		       "\033[%s%s%sm" % \
		       ("" if fgcolor is None else ("38;5;%d" % fgcolor), \
		       	"" if bgcolor is None else (";48;5;%d" % bgcolor), \
		       	";1" if bold else "")
	return ""

# Output colors as xterm color codes
# (see e.g. http://www.calmar.ws/vim/256-xterm-24bit-rgb-color-chart.html)
foreground_color = 251
heading_color = 208
heading_line_color = 246
time_color = 231
time_text_color = 246
histogram_color = [ 57, 56, 126, 197 ]
histogram_color_grid = [ 99, 97, 169, 204 ]
weekday_color = 245
weekend_color = 231
bar_color = 28
bar_weekend_color = 82
bar_color_grid = 77
bar_weekend_color_grid = 156
night_color = 51
sunrise_color = 228
noon_color = 226
sunset_color = 214
grid_color = 238

# Returns a string that, if printed to the terminal,
# will display the specified text with the specified attributes
def style_text(text, fgcolor = foreground_color, bgcolor = None, bold = False):
	return get_escape_sequence(fgcolor, bgcolor, bold) + text + \
	       get_escape_sequence(fgcolor = foreground_color)


# Computes the number of hours (total), minutes and seconds
# in the specified timedelta object
def get_delta_fields(delta):
	fields = {}
	fields["hours"], remainder = divmod(round(delta.total_seconds()), 3600)
	fields["minutes"], fields["seconds"] = divmod(remainder, 60)
	return fields

# Formats the specified timedelta object into a string
# of the form "X hours Y minutes"
def format_delta(delta):
	fields = get_delta_fields(delta)
	return style_text("%4d" % fields["hours"], fgcolor = time_color) + \
	       style_text(" hours ", fgcolor = time_text_color) + \
	       style_text("%2d" % fields["minutes"], fgcolor = time_color) + \
	       style_text(" minutes", fgcolor = time_text_color)

# Formats the specified timedelta object into a string
# of the form "XX:YY"
def format_delta_short(delta):
	fields = get_delta_fields(delta)
	if fields["hours"] == 0 and fields["minutes"] == 0:
		return ""
	if fields["hours"] == 0:
		return style_text("  :", fgcolor = time_text_color) + \
		       style_text("%02d" % fields["minutes"], fgcolor = time_color)
	return style_text("%2d" % fields["hours"], fgcolor = time_color) + \
	       style_text(":", fgcolor = time_text_color) + \
		   style_text("%02d" % fields["minutes"], fgcolor = time_color)

output_width = 61

# Returns a styled section separator used to frame a calendar month
def format_heading(heading):
	padded_heading = " " + heading + " "
	heading_pos = int((output_width - len(padded_heading)) / 2)
	full_heading = style_text("┌" + ("─" * heading_pos), fgcolor = heading_line_color) + \
	               style_text(padded_heading, fgcolor = heading_color)
	full_heading += style_text(("─" * (output_width - heading_pos - len(padded_heading))) + "┐", \
		                       fgcolor = heading_line_color)
	return full_heading


def format_centered(text, color):
	text_len = len(text)
	left_pad = max(0, int((output_width - text_len) / 2))
	return (" " * left_pad) + style_text(text, fgcolor = color)



def is_binary_file(path):
	try:
		with open(path, "rb") as f:
			chunk = f.read(1024)
			return b"\0" in chunk
	except Exception:
		return False


def fetch_remote_lines(host):
	try:
		res = subprocess.run(["ssh", host, "uname -s"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True)
		if res.returncode != 0:
			sys.exit("Could not connect to '%s' via SSH: %s" % (host, res.stderr.strip()))
		remote_os = res.stdout.strip()
	except FileNotFoundError:
		sys.exit("Error: 'ssh' command not found.")

	# Try remote 'ranwhen --raw' first
	try:
		raw_res = subprocess.run(["ssh", host, "ranwhen --raw 2>/dev/null"], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, universal_newlines=True)
		if raw_res.returncode == 0 and raw_res.stdout.strip():
			lines = raw_res.stdout.splitlines()
			if lines:
				return lines
	except Exception:
		pass

	if remote_os == "Darwin":
		# macOS fallback: run ranwhen --raw if possible or error
		sys.exit("Remote macOS host '%s' requires ranwhen installed on the target." % host)
	else:
		try:
			last_res = subprocess.run(["ssh", host, "last -R -F reboot"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, universal_newlines=True)
			if last_res.returncode != 0:
				sys.exit("Failed to retrieve reboot history from '%s': %s" % (host, last_res.stderr.strip()))
			lines = last_res.stdout.splitlines()
			if not lines:
				sys.exit("No reboot history found on '%s'" % host)
			return lines
		except Exception as e:
			sys.exit("Failed to run 'last' on '%s': %s" % (host, e))


def get_input_lines():
	parser = argparse.ArgumentParser(
		description = "ranwhen – Visualize when your system was running"
	)
	parser.add_argument(
		"file",
		nargs = "?",
		help = "Path to a text log file, binary wtmp file, or remote host name. Use '-' for stdin.",
	)
	parser.add_argument(
		"-f", "--file-wtmp",
		dest = "wtmp_file",
		help = "Explicit binary wtmp file to pass to 'last -f'",
	)
	args = parser.parse_args()

	# 1. Explicit -f / --file-wtmp argument
	if args.wtmp_file:
		cmd = ["last", "-R", "-F", "reboot", "-f", args.wtmp_file]
		try:
			output = subprocess.check_output(cmd, universal_newlines = True)
			return output.splitlines()
		except FileNotFoundError:
			sys.exit("Error: 'last' command not found. Please ensure util-linux is installed.")
		except subprocess.CalledProcessError as e:
			sys.exit("Error running '%s': %s" % (" ".join(cmd), e))

	# 2. Positional argument: file, remote host, or stdin '-'
	if args.file:
		if args.file == "-":
			return sys.stdin.read().splitlines()
		if not os.path.exists(args.file):
			return fetch_remote_lines(args.file)
		if is_binary_file(args.file):
			cmd = ["last", "-R", "-F", "reboot", "-f", args.file]
			try:
				output = subprocess.check_output(cmd, universal_newlines = True)
				return output.splitlines()
			except FileNotFoundError:
				sys.exit("Error: 'last' command not found. Please ensure util-linux is installed.")
			except subprocess.CalledProcessError as e:
				sys.exit("Error running '%s': %s" % (" ".join(cmd), e))
		else:
			try:
				with open(args.file, "r", encoding = "utf-8", errors = "replace") as f:
					return f.read().splitlines()
			except Exception as e:
				sys.exit("Error reading file '%s': %s" % (args.file, e))

	# 3. Piped stdin (e.g. cat file | ./ranwhen.py)
	if not sys.stdin.isatty():
		lines = sys.stdin.read().splitlines()
		if lines:
			return lines

	# 4. Default: discover existing wtmp files
	wtmp_candidates = ["/var/log/wtmp"]
	i = 1
	while os.path.exists("/var/log/wtmp.%d" % i):
		wtmp_candidates.append("/var/log/wtmp.%d" % i)
		i += 1

	existing_wtmp = [f for f in wtmp_candidates if os.path.exists(f)]
	cmd = ["last", "-R", "-F", "reboot"]
	if len(existing_wtmp) > 1:
		for f in existing_wtmp:
			cmd.extend(["-f", f])
	elif len(existing_wtmp) == 1 and existing_wtmp[0] != "/var/log/wtmp":
		cmd.extend(["-f", existing_wtmp[0]])

	try:
		output = subprocess.check_output(cmd, universal_newlines = True)
		return output.splitlines()
	except FileNotFoundError:
		sys.exit("Error: 'last' command not found. Please ensure util-linux is installed.")
	except subprocess.CalledProcessError as e:
		sys.exit("Error running '%s': %s" % (" ".join(cmd), e))


##### Program logic starts here #####

# Major time granularity (lines)
day = timedelta(days = 1)

# Minor time granularity (columns)
half_hour = timedelta(minutes = 30)

# Ratio of granularities (columns per line)
half_hours_in_day = round(day / half_hour)

lines = get_input_lines()

now = datetime.now().replace(microsecond = 0)
time_spans = []

### Parse output
for line in lines:
	result = parse_line(line, now = now)
	if result is not None:
		time_spans.append(result)

if not time_spans:
	sys.exit("Error: No parsable reboot records found.")


### Sort and merge overlapping time spans
time_spans.sort(key = lambda s: s["from"])

merged = []
for span in time_spans:
	if not merged:
		merged.append(span)
	else:
		prev = merged[-1]
		if span["from"] <= prev["to"]:
			prev["to"] = max(prev["to"], span["to"])
		else:
			merged.append(span)

time_spans = merged


### Compute period
latest_time   = max(s["to"] for s in time_spans).replace(hour = 0, minute = 0, second = 0, microsecond = 0) + day
earliest_time = min(s["from"] for s in time_spans).replace(hour = 0, minute = 0, second = 0, microsecond = 0)


### Compute runtime for each half hour time slot in period
time_slots = []
aggregated_time_slots = [ timedelta() ] * half_hours_in_day
total_time = timedelta()

# Fast sweep-line slot overlap computation
n_spans = len(time_spans)
span_idx = 0

current_time = earliest_time
while current_time < latest_time:
	slot_end = current_time + half_hour
	while span_idx < n_spans and time_spans[span_idx]["to"] <= current_time:
		span_idx += 1
	time_in_slot = timedelta()
	i = span_idx
	while i < n_spans and time_spans[i]["from"] < slot_end:
		overlap = min(slot_end, time_spans[i]["to"]) - max(current_time, time_spans[i]["from"])
		if overlap > timedelta():
			time_in_slot += overlap
		i += 1

	time_slots.append({ "time" : current_time, "time_in_slot" : time_in_slot })
	half_hour_index = current_time.hour * 2 + (1 if current_time.minute >= 30 else 0)
	aggregated_time_slots[half_hour_index] += time_in_slot
	total_time += time_in_slot
	current_time = slot_end


# Group time slots by date for fast daily chart rendering
slots_by_date = {}
for slot in time_slots:
	d = slot["time"].date()
	if d not in slots_by_date:
		slots_by_date[d] = []
	slots_by_date[d].append(slot)


time_header = "       0:00 " + style_text("☾", fgcolor = night_color) + \
			  "      6:00 " + style_text("☀", fgcolor = sunrise_color) + \
			  "     12:00 " + style_text("☀", fgcolor = noon_color) + \
			  "     18:00 " + style_text("☀", fgcolor = sunset_color) + \
			  "     24:00 " + style_text("☽", fgcolor = night_color)

grid_header = style_text("▆           ▆           ▆           ▆           ▆", fgcolor = grid_color)
grid_footer = style_text("▀           ▀           ▀           ▀           ▀", fgcolor = grid_color)
level_characters = [ " ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█" ]


# Set default foreground color for output
print(get_escape_sequence(fgcolor = foreground_color), end = "")


### Group days into months and detect empty months
months = []
cur = earliest_time

while cur < latest_time:
	month_name = cur.strftime("%B %Y")
	day_active = any(slot["time_in_slot"] > timedelta(0) for slot in slots_by_date.get(cur.date(), []))

	if months and months[-1]["name"] == month_name:
		months[-1]["days"].append(cur)
		if day_active:
			months[-1]["has_activity"] = True
	else:
		months.append({
			"name": month_name,
			"days": [cur],
			"has_activity": day_active,
		})
	cur += day

levels = len(level_characters) - 2
m_idx = 0

while m_idx < len(months):
	if not months[m_idx]["has_activity"]:
		empty_start = m_idx
		while m_idx < len(months) and not months[m_idx]["has_activity"]:
			m_idx += 1
		empty_count = m_idx - empty_start
		print()
		print()
		if empty_count == 1:
			print(format_heading(months[empty_start]["name"]))
			print(format_centered("── no activity ──", grid_color))
		else:
			first_name = months[empty_start]["name"]
			last_name = months[m_idx - 1]["name"]
			range_title = f"{first_name} – {last_name}"
			count_str = f"── {empty_count} months with no activity ──"
			print(format_heading(range_title))
			print(format_centered(count_str, grid_color))
		print("        " + grid_footer)
		continue

	month = months[m_idx]
	print()
	print()
	print(format_heading(month["name"]))
	print()
	print(time_header)
	print("        " + grid_header)

	for current_time in month["days"]:
		weekend = current_time.weekday() in [5, 6]
		sunday  = current_time.weekday() == 6

		time_text = style_text(current_time.strftime("%a"), \
			                   fgcolor = weekend_color if weekend else weekday_color, \
			                   bold = sunday)
		time_text += current_time.strftime(" %d").replace(" 0", "  ")

		output_line = time_text + "  "

		time_sum = timedelta()

		bar_text = ""

		slot_index = 0

		### Build chart for day
		for time_slot in slots_by_date.get(current_time.date(), []):
			time_sum += time_slot["time_in_slot"]
			level = round((time_slot["time_in_slot"] / half_hour) * levels)
			grid = slot_index % 12 == 0
			slot_index += 1
			bar_text += style_text(level_characters[level], \
				                   fgcolor = (bar_weekend_color_grid if grid else bar_weekend_color) if weekend \
				                             else (bar_color_grid if grid else bar_color), \
				                   bgcolor = grid_color if grid else None)

		output_line += bar_text

		output_line += style_text(" ", bgcolor = grid_color)

		output_line += " " + format_delta_short(time_sum)

		print(output_line)

	print("        " + grid_footer)
	m_idx += 1


### Print summary
number_of_days = max(1, (latest_time - earliest_time).days)

print()
print()
print(style_text("Period:  ", bold = True) + \
	  earliest_time.strftime("%B %d %Y") + " – " + \
	  (latest_time - day).strftime("%B %d %Y") + \
	  " (" + \
	  style_text("%d" % number_of_days, fgcolor = time_color) + \
	  style_text(" days", fgcolor = time_text_color) + ")")

print()

print(style_text("Total time running: ", bold = True) + format_delta(total_time))
print(style_text("Daily average:      ", bold = True) + format_delta(total_time / number_of_days))

print()
print()


### Print histogram
print(style_text("Histogram:", bold = True))
print()
print(time_header)
print("   max  " + grid_header)

number_of_lines = 4

levels = len(level_characters) - 1

min_level = (min(aggregated_time_slots) / number_of_days) / half_hour
max_level = (max(aggregated_time_slots) / number_of_days) / half_hour
level_range = max_level - min_level

def format_histogram_line(label, index):
	line = label + "  "
	slot_index = 0
	for time_slot in aggregated_time_slots:
		level = (time_slot / number_of_days) / half_hour
		# Normalize level to increase resolution
		level = ((level - min_level) / level_range) if level_range > 0 else 0
		level = round(level * (levels * number_of_lines)) - (index * levels)
		# Clamp level to permissible range
		level = max(0, min(levels, level))
		grid = slot_index % 12 == 0
		slot_index += 1
		line += style_text(level_characters[level], \
			               fgcolor = histogram_color_grid[index] if grid else histogram_color[index], \
			               bgcolor = grid_color if grid else None)
	line += style_text(" ", bgcolor = grid_color)
	return line

print(format_histogram_line("      ", 3))
print(format_histogram_line("      ", 2))
print(format_histogram_line("      ", 1))
print(format_histogram_line("   min", 0))

print("        " + grid_footer)

print()


# Reset text attributes
print(get_reset_sequence(), end = "")
