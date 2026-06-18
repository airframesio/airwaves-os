#!/bin/bash
# airwaves-tui-lib.sh — shared helpers for the Airwaves OS console (airwaves-tui)
# and installer (airwaves-firstrun) so both render the SAME branded look:
#   * the night-watch brand palette (remapped into the 16 console slots),
#   * one flat amber-accented dialog theme,
#   * a centered wordmark banner, and
#   * \Z-colour-aware, UTF-8-aware centering helpers.
# Sourced (not executed). Ships in /opt/airwaves/scripts alongside the scripts.
#
# Colour discipline (one meaning per colour, after the palette remap below):
#   amber = look-here/active · green = healthy · ember = danger
#   slate = quiet label/helper · cream = the value · rose/violet = brand chrome only
# Every dialog box is called with --colors; lead colour runs with \Zn, close \Zb
# with \ZB, and SANITIZE interpolated values (cz) so untrusted text can't inject \Z.

# ---- locale: make ${#str} count COLUMNS, not bytes -------------------------
# center_block/center_lines measure with ${#}. Under a C/POSIX locale that counts
# BYTES, so a box-drawing banner (multibyte rules) would shear. Force a UTF-8
# locale if one is available; otherwise AW_UTF8=0 and the banner uses pure ASCII.
AW_UTF8=0
aw_setup_locale() {
    AW_UTF8=0
    local avail cur l
    avail="$(locale -a 2>/dev/null)"
    # Trust the current locale ONLY if it claims UTF-8 AND is actually generated
    # (glibc silently falls back to byte-semantics C if the named locale is absent,
    # which would make ${#str} miscount and shear the multibyte banner/glyphs).
    case "${LC_ALL:-}${LC_CTYPE:-}${LANG:-}" in
        *[Uu][Tt][Ff]*)
            cur="${LC_ALL:-${LC_CTYPE:-${LANG:-}}}"
            if printf '%s\n' "$avail" | grep -qix "$cur"; then AW_UTF8=1; return 0; fi
            ;;
    esac
    # Otherwise adopt a known UTF-8 locale that IS generated.
    for l in C.UTF-8 C.utf8 en_US.UTF-8 en_US.utf8; do
        if printf '%s\n' "$avail" | grep -qix "$l"; then
            export LC_ALL="$l"; AW_UTF8=1; return 0
        fi
    done
    return 0
}

# ---- console hygiene -------------------------------------------------------
# Stop kernel log messages (printk) from clobbering the dialog UI; dmesg keeps them.
quiet_console() { dmesg -n 1 2>/dev/null || echo 1 > /proc/sys/kernel/printk 2>/dev/null || true; }

# ---- brand palette: remap the 16 console slots to the night-watch RGB ------
# Only on a real Linux VT (TERM=linux + a tty); a no-op everywhere else (serial,
# ssh, the build host). After this, the standard dialog colour NAMES carry brand
# hex (BLACK=ink, YELLOW=amber, WHITE=cream-dim, bright WHITE=cream, GREEN=ok,
# CYAN=slate, RED=ember, bright BLACK=hairline, BLUE=scene-blue, MAGENTA=rose).
aw_palette() {
    [ "${TERM:-}" = linux ] || return 0
    [ -t 1 ] || return 0
    {
        printf '\033]P0070a10\033]P1ff6a3d\033]P23a9d68\033]P3ffa14f'
        printf '\033]P4181a30\033]P5fc4a67\033]P68d93a2\033]P7c4bfb2'
        printf '\033]P83a4150\033]P9ff8a63\033]Pa59d98c\033]Pbffc372'
        printf '\033]Pc252a48\033]Pd8e54e9\033]Peb0b5c2\033]Pfece5d6'
        printf '\033[2J\033[H'      # repaint so the new ink bg fills the screen
    } > /dev/tty 2>/dev/null || true
}
# Restore the console's default palette (before handing back to a plain shell).
aw_restore_palette() {
    [ "${TERM:-}" = linux ] || return 0
    printf '\033]R' > /dev/tty 2>/dev/null || true
}

# ---- measuring & centering (\Z-aware, UTF-8-aware) -------------------------
# Visible width of a string: drop dialog \Z colour codes (\Z + one char) first;
# the remaining length is columns under a UTF-8 locale (see aw_setup_locale).
vis_len() { local s="${1:-}"; s="${s//\\Z?/}"; printf '%s' "${#s}"; }

# Center a whole block by its widest VISIBLE line, padding every line equally so
# internal alignment (banner art, aligned columns) is preserved.
center_block() {
    local w="$1" max=0 v pad; local -a lines=(); local line
    while IFS= read -r line; do lines+=("$line"); v="$(vis_len "$line")"; [ "$v" -gt "$max" ] && max="$v"; done
    pad=$(( (w - max) / 2 )); [ "$pad" -lt 0 ] && pad=0
    for line in "${lines[@]}"; do printf '%*s%s\n' "$pad" '' "$line"; done
}
# Center each line independently to width $1 (for single-line details).
center_lines() {
    local w="$1" line v pad
    while IFS= read -r line; do
        v="$(vis_len "$line")"; pad=$(( (w - v) / 2 )); [ "$pad" -lt 0 ] && pad=0
        printf '%*s%s\n' "$pad" '' "$line"
    done
}

# ---- console dimensions ----------------------------------------------------
# Read the LIVE terminal winsize. Must work inside $(...) — where tput's stdout is
# a pipe, so it can't ioctl the tty and falls back to the 80x24 terminfo default
# (which would silently shrink/clip the layout). stty reading /dev/tty (the
# controlling terminal) is reliable there; tput/$LINES are last-resort fallbacks.
_aw_size() {   # echoes "rows cols"
    local s
    s="$(stty size < /dev/tty 2>/dev/null)" && [ -n "$s" ] && { printf '%s' "$s"; return; }
    s="$(stty size 2>/dev/null)" && [ -n "$s" ] && { printf '%s' "$s"; return; }
    printf '%s %s' "$(tput lines 2>/dev/null || echo 25)" "$(tput cols 2>/dev/null || echo 80)"
}
scr_cols() { local r c; read -r r c < <(_aw_size); { [ -n "${c:-}" ] && [ "$c" -gt 0 ] 2>/dev/null; } && echo "$c" || echo 80; }
scr_rows() { local r c; read -r r c < <(_aw_size); { [ -n "${r:-}" ] && [ "$r" -gt 0 ] 2>/dev/null; } && echo "$r" || echo 25; }
# A generous, readable slice of the console, floored so it stays usable at 80 cols.
box_width() { local c w; c="$(scr_cols)"; w=$(( c * 92 / 100 )); [ "$w" -gt 92 ] && w=92; [ "$w" -lt 52 ] && w=52; echo "$w"; }

# ---- sanitize an interpolated value for a --colors string ------------------
# Strip backslashes so a literal "\Z" inside untrusted data (SSID, hostname,
# error text, disk model) can't inject a colour code.
cz() { local s="${1:-}"; printf '%s' "${s//\\/}"; }

# Human-readable system uptime, recomputed at every screen draw (so it is current
# each time the console renders — no flickering background refresh needed).
uptime_str() {
    awk '{d=int($1/86400);h=int(($1%86400)/3600);m=int(($1%3600)/60);
          if(d)printf "%dd %dh %dm",d,h,m; else if(h)printf "%dh %dm",h,m; else printf "%dm",m}' \
        /proc/uptime 2>/dev/null
}

# ---- wordmark banner -------------------------------------------------------
# The big "AIRWAVES OS" wordmark: bold amber block letters on a filled shadow
# panel (the ░ texture outlines the letters and fills the gaps — a BBS-style
# plaque). Baked at build time into config/banner.txt (\Z-coloured), so there is
# no runtime figlet/toilet dependency and the art is swappable. Shown only when
# UTF-8 is available and the box is wide enough to hold it; otherwise a compact
# letter-spaced fallback. $1 = inner box width.
AW_BANNER="${AW_BANNER:-/opt/airwaves/config/banner.txt}"
banner_art() {
    local iw="${1:-60}" line v w=0
    if [ "${AW_UTF8:-0}" = 1 ] && [ -r "$AW_BANNER" ]; then
        while IFS= read -r line; do v="$(vis_len "$line")"; [ "$v" -gt "$w" ] && w="$v"; done < "$AW_BANNER"
        if [ "$w" -gt 0 ] && [ "$iw" -ge "$w" ]; then cat "$AW_BANNER"; return; fi
    fi
    # Compact fallback: narrow console, no UTF-8, or banner file missing.
    local pip
    if [ "${AW_UTF8:-0}" = 1 ]; then pip='\Z5((\Zb\Z5•\ZB\Z5))\Zn'; else pip='\Zb\Z5((o))\Zn'; fi
    printf '%s\n' "${pip}  \Zb\Z7A I R W A V E S   O S\Zn" "\Z6the listening station\Zn"
}

# ---- hand-drawn frame ------------------------------------------------------
# Draw a box-drawing frame of total width $1 around the piped content, with an
# amber centred title on the top border ($2) and one blank padding row inside the
# top and bottom. Used to give a bordered box inside a borderless (hero) infobox
# — full control over padding/alignment. Content lines may carry \Z codes (their
# width is measured with vis_len). Hairline border, amber title, cream content.
framebox() {
    local W="$1" title="${2:-}" iw cl cr tl tr cm vl line v pad mid l r space
    iw=$(( W - 2 ))            # columns between the vertical borders
    if [ "${AW_UTF8:-0}" = 1 ]; then cl='┌'; cr='┐'; tl='└'; tr='┘'; cm='─'; vl='│'; else cl='+'; cr='+'; tl='+'; tr='+'; cm='-'; vl='|'; fi
    # top border, with a centred title if given:  ┌──── Title ────┐
    v="$(vis_len "$title")"
    if [ "$v" -gt 0 ]; then
        space=$(( iw - v - 2 )); [ "$space" -lt 0 ] && space=0
        l=$(( space / 2 )); r=$(( space - l ))
        local ml mr; printf -v ml '%*s' "$l" ''; ml="${ml// /$cm}"; printf -v mr '%*s' "$r" ''; mr="${mr// /$cm}"
        printf '\Zb\Z0%s%s\Zn \Zb\Z3%s\Zn \Zb\Z0%s%s\Zn\n' "$cl" "$ml" "$title" "$mr" "$cr"
    else
        printf -v mid '%*s' "$iw" ''; mid="${mid// /$cm}"; printf '\Zb\Z0%s%s%s\Zn\n' "$cl" "$mid" "$cr"
    fi
    printf '\Zb\Z0%s\Zn%*s\Zb\Z0%s\Zn\n' "$vl" "$iw" '' "$vl"   # top padding row
    while IFS= read -r line; do
        v="$(vis_len "$line")"; pad=$(( iw - 2 - v )); [ "$pad" -lt 0 ] && pad=0
        printf '\Zb\Z0%s\Zn  %s%*s\Zb\Z0%s\Zn\n' "$vl" "$line" "$pad" '' "$vl"
    done
    printf '\Zb\Z0%s\Zn%*s\Zb\Z0%s\Zn\n' "$vl" "$iw" '' "$vl"   # bottom padding row
    printf -v mid '%*s' "$iw" ''; mid="${mid// /$cm}"; printf '\Zb\Z0%s%s%s\Zn\n' "$tl" "$mid" "$tr"
}

# ---- canonical dialog theme (fallback) -------------------------------------
# The shared theme file (config/dialogrc -> /opt/airwaves/config/dialogrc) is the
# primary. This prints a byte-identical copy so a script can recreate it on tmpfs
# if the shared file is ever missing — keeping installer + console in lockstep.
aw_dialogrc() {
cat <<'RC'
use_shadow = OFF
use_colors = ON
screen_color           = (WHITE,BLACK,OFF)
shadow_color           = (BLACK,BLACK,OFF)
dialog_color           = (WHITE,BLACK,OFF)
title_color            = (YELLOW,BLACK,ON)
border_color           = (BLACK,BLACK,ON)
border2_color          = (BLACK,BLACK,ON)
menubox_color          = (WHITE,BLACK,OFF)
menubox_border_color   = (BLACK,BLACK,ON)
menubox_border2_color  = (BLACK,BLACK,ON)
item_color             = (WHITE,BLACK,OFF)
item_selected_color    = (BLACK,YELLOW,OFF)
tag_color              = (CYAN,BLACK,OFF)
tag_selected_color     = (BLACK,YELLOW,OFF)
tag_key_color          = (YELLOW,BLACK,ON)
tag_key_selected_color = (BLACK,YELLOW,ON)
button_active_color         = (BLACK,YELLOW,OFF)
button_inactive_color       = (CYAN,BLACK,OFF)
button_key_active_color     = (BLACK,YELLOW,ON)
button_key_inactive_color   = (YELLOW,BLACK,ON)
button_label_active_color   = (BLACK,YELLOW,ON)
button_label_inactive_color = (WHITE,BLACK,OFF)
inputbox_color           = (WHITE,BLUE,OFF)
inputbox_border_color    = (BLACK,BLACK,ON)
inputbox_border2_color   = (BLACK,BLACK,ON)
form_active_text_color   = (BLACK,YELLOW,OFF)
form_text_color          = (WHITE,BLUE,OFF)
form_item_readonly_color = (CYAN,BLACK,OFF)
searchbox_color          = (WHITE,BLUE,OFF)
searchbox_title_color    = (YELLOW,BLACK,ON)
searchbox_border_color   = (BLACK,BLACK,ON)
searchbox_border2_color  = (BLACK,BLACK,ON)
position_indicator_color = (YELLOW,BLACK,ON)
uarrow_color             = (CYAN,BLACK,OFF)
darrow_color             = (CYAN,BLACK,OFF)
itemhelp_color           = (CYAN,BLACK,OFF)
gauge_color              = (YELLOW,BLACK,ON)
RC
}
