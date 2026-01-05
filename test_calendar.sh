#!/bin/bash
current_year=$(date +%Y)
current_month=$(date +%m)
current_day=$(date +%d)

# Strip leading zeros for AppleScript
year=$((10#$current_year))
month=$((10#$current_month))
day=$((10#$current_day))

osascript -e "
tell application \"Calendar\"
    activate
    switch view to day view
    set targetDate to current date
    set year of targetDate to $year
    set month of targetDate to $month
    set day of targetDate to $day
    set time of targetDate to (14 * 3600 + 0 * 60)
    switch view to targetDate
end tell
"
