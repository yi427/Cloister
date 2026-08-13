#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="$repo_root/docs/assets"
font_path="/System/Library/Fonts/SFNSMono.ttf"

if ! command -v magick >/dev/null 2>&1; then
    echo "ImageMagick is required (brew install imagemagick)." >&2
    exit 1
fi

if [[ ! -f "$font_path" ]]; then
    echo "Required macOS font not found: $font_path" >&2
    exit 1
fi

mkdir -p "$output_dir"

demo_root="$(mktemp -d "${TMPDIR:-/tmp}/cloister-preview.XXXXXX")"
trap 'rm -rf "$demo_root"' EXIT
mkdir -p "$demo_root/project"

(
    cd "$repo_root"
    cargo build --quiet
)

dry_run_output="$(
    cd "$repo_root"
    XDG_CONFIG_HOME="$demo_root/config" \
        XDG_DATA_HOME="$demo_root/data" \
        XDG_STATE_HOME="$demo_root/state" \
        target/debug/cloister codex \
        --profile examples/profile.toml \
        --workspace "$demo_root/project" \
        --dry-run
)"

# Keep only the most useful dry-run contract lines. The workspace path is the
# sole normalization: the real disposable directory is presented as ~/demo so
# the preview remains privacy-safe and readable.
preview_lines=()
while IFS= read -r line; do
    case "$line" in
        "Profile:"* | \
            "Runtime:"* | \
            "Root filesystem:"* | \
            "Network:"* | \
            "Guest proxy:"* | \
            "SSH agent forwarding:"* | \
            "Host credential mounts:"* | \
            "Host bridge:"* | \
            "Host policy:"* | \
            "Codex MCP approval:"* | \
            "Host bridge token:"* | \
            "Agent:"* | \
            "Lifecycle:"*)
            preview_lines+=("$line")
            ;;
        "Host capabilities:"*)
            preview_lines+=("Host capabilities: host.list_commands, host.exec,")
            preview_lines+=("  host.exec_status, host.exec_cancel (Profile-governed; macOS user permissions)")
            ;;
        "Workspace:"*)
            preview_lines+=("Workspace: ~/demo -> /workspace (read-write)")
            ;;
    esac
done <<<"$dry_run_output"

if [[ "${#preview_lines[@]}" -ne 16 ]]; then
    echo "Unexpected dry-run output: wanted 16 preview lines, found ${#preview_lines[@]}." >&2
    exit 1
fi

canvas="#070b10"
panel="#0d141c"
panel_border="#263443"
titlebar="#111b25"
text_color="#c9d4df"
muted="#718194"
accent="#68d8d2"
green="#7ce3ad"
gold="#f0c674"

width=1440
height=810
command_text='$ cloister codex --profile examples/profile.toml --workspace ~/demo --dry-run'
frame_dir="$demo_root/frames"
mkdir -p "$frame_dir"

line_color() {
    case "$1" in
        "Network:"* | "Workspace:"*) printf '%s' "$gold" ;;
        "Host credential mounts:"* | "Host bridge token:"*) printf '%s' "$green" ;;
        "Host bridge:"* | "Host capabilities:"* | "  host.exec_status"* | \
            "Codex MCP approval:"* | "Agent:"*)
            printf '%s' "$accent"
            ;;
        *) printf '%s' "$text_color" ;;
    esac
}

render_frame() {
    local output_path="$1"
    local shown_command="$2"
    local visible_lines="$3"
    local show_cursor="$4"
    local y=192
    local index=0
    local line
    local color
    local -a args

    args=(
        -size "${width}x${height}" "xc:$canvas"
        -fill "$panel" -stroke "$panel_border" -strokewidth 2
        -draw "roundrectangle 28,24 1412,786 20,20"
        -fill "$titlebar" -stroke none
        -draw "roundrectangle 29,25 1411,94 19,19"
        -draw "rectangle 29,70 1411,94"
        -fill "#ff6b6b" -draw "circle 62,59 69,59"
        -fill "#f5c451" -draw "circle 88,59 95,59"
        -fill "#62d394" -draw "circle 114,59 121,59"
        -font "$font_path" -pointsize 18 -fill "$muted" -gravity NorthWest
        -annotate +598+49 "cloister 0.2.0  ·  dry-run"
        -pointsize 17 -fill "$accent"
        -annotate +982+49 "PROFILE-GOVERNED AI AGENTS"
        -pointsize 23 -fill "$accent"
        -annotate +70+122 "$shown_command"
    )

    if [[ "$show_cursor" == "yes" ]]; then
        local cursor_x=$((70 + ${#shown_command} * 14 + 4))
        args+=(
            -fill "$accent" -stroke none
            -draw "rectangle ${cursor_x},120 $((cursor_x + 11)),147"
        )
    fi

    for line in "${preview_lines[@]}"; do
        if (( index >= visible_lines )); then
            break
        fi
        color="$(line_color "$line")"
        args+=(
            -font "$font_path" -pointsize 22 -fill "$color" -gravity NorthWest
            -annotate "+70+${y}" "$line"
        )
        y=$((y + 33))
        index=$((index + 1))
    done

    args+=(
        -font "$font_path" -pointsize 17 -fill "$muted" -gravity NorthWest
        -annotate +70+752 "explicit mounts  ·  authenticated host bridge  ·  ephemeral token redacted"
        "$output_path"
    )

    magick "${args[@]}"
}

render_frame "$frame_dir/00.png" '$' 0 yes
render_frame "$frame_dir/01.png" '$ cloister' 0 yes
render_frame "$frame_dir/02.png" '$ cloister codex --profile examples/profile.toml' 0 yes
render_frame "$frame_dir/03.png" "$command_text" 0 yes
render_frame "$frame_dir/04.png" "$command_text" 3 no
render_frame "$frame_dir/05.png" "$command_text" 8 no
render_frame "$frame_dir/06.png" "$command_text" 13 no
render_frame "$frame_dir/07.png" "$command_text" 16 no

magick "$frame_dir/07.png" \
    -depth 8 -strip \
    -define png:exclude-chunks=date,time \
    -define png:compression-level=9 \
    "$output_dir/terminal-trove-preview.png"

magick \
    -delay 55 "$frame_dir/00.png" \
    -delay 45 "$frame_dir/01.png" \
    -delay 70 "$frame_dir/02.png" \
    -delay 100 "$frame_dir/03.png" \
    -delay 90 "$frame_dir/04.png" \
    -delay 100 "$frame_dir/05.png" \
    -delay 115 "$frame_dir/06.png" \
    -delay 380 "$frame_dir/07.png" \
    -loop 0 -layers OptimizeTransparency -colors 128 -strip \
    "$output_dir/terminal-trove-preview.gif"

# GitHub renders repository social previews at a 2:1 aspect ratio. Keep this
# composition separate from the denser Terminal Trove capture so its title and
# security-boundary summary remain legible in link cards.
social_text="#e1e9f2"
social_muted="#9aabba"
magick \
    -size 1280x640 "xc:$canvas" \
    -fill "#0a1119" -stroke none \
    -draw "circle 146,560 420,560" \
    -fill "$accent" \
    -draw "roundrectangle 62,58 170,88 15,15" \
    -font "$font_path" -pointsize 15 -fill "$canvas" -gravity NorthWest \
    -annotate +80+65 "CLOISTER" \
    -pointsize 45 -fill "$social_text" -stroke "$social_text" -strokewidth 1 \
    -annotate +60+120 "Explicit" \
    -annotate +60+174 "environments for" \
    -annotate +60+228 "AI coding agents" \
    -stroke none -pointsize 19 -fill "$social_muted" \
    -annotate +64+286 "Codex + Claude Code" \
    -annotate +64+314 "inside Apple container" \
    -pointsize 20 -fill "$green" \
    -annotate +66+390 "●  Apple silicon macOS" \
    -annotate +66+434 "●  Terminal-first Rust CLI" \
    -annotate +66+478 "●  Profile-governed host access" \
    -fill "$panel" -stroke "$panel_border" -strokewidth 2 \
    -draw "roundrectangle 520,54 1224,586 18,18" \
    -fill "$titlebar" -stroke none \
    -draw "roundrectangle 521,55 1223,112 17,17" \
    -draw "rectangle 521,88 1223,112" \
    -fill "#ff6b6b" -draw "circle 550,83 556,83" \
    -fill "#f5c451" -draw "circle 573,83 579,83" \
    -fill "#62d394" -draw "circle 596,83 602,83" \
    -font "$font_path" -pointsize 17 -fill "$social_muted" \
    -annotate +801+73 "cloister · dry-run" \
    -pointsize 24 -fill "$accent" -stroke "$accent" -strokewidth 0.5 \
    -annotate +552+136 '$ cloister codex --dry-run' \
    -stroke none -pointsize 22 -fill "$social_text" \
    -annotate +552+201 "${preview_lines[2]}" \
    -fill "$gold" \
    -annotate +552+249 "${preview_lines[5]}" \
    -fill "$green" \
    -annotate +552+297 "${preview_lines[7]}" \
    -annotate +552+345 "Host bridge token: ephemeral, forwarded," \
    -annotate +552+383 "  and redacted" \
    -fill "$accent" \
    -annotate +552+431 "${preview_lines[14]}" \
    -fill "$social_text" \
    -annotate +552+479 "${preview_lines[15]}" \
    -pointsize 18 -fill "$social_muted" \
    -annotate +552+535 "profile-governed · reviewable · fail-closed" \
    -depth 8 -strip \
    -define png:exclude-chunks=date,time \
    -define png:compression-level=9 \
    "$output_dir/github-social-preview.png"

echo "Rendered:"
echo "  $output_dir/terminal-trove-preview.png"
echo "  $output_dir/terminal-trove-preview.gif"
echo "  $output_dir/github-social-preview.png"
