default:
    @just --list

# Run against the local test wallpaper
run: test-video
    cargo run --release -- ./wallpapers/bg.mp4

# Check formatting, lints, and types without building
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings

# Generate a small lossless test clip (skips if wallpapers/bg.mp4 already exists)
test-video resolution="1280x720":
    #!/usr/bin/env sh
    if [ -f wallpapers/bg.mp4 ]; then
        echo "wallpapers/bg.mp4 already exists, skipping (use 'just retest-video' to force regen)"
        exit 0
    fi
    mkdir -p wallpapers
    ffmpeg -f lavfi -i "mandelbrot=s={{resolution}}:rate=30" -t 8 -pix_fmt yuv420p \
        -c:v libx264 -crf 0 -preset veryslow wallpapers/bg.mp4 -y

# Force-regenerate the test clip regardless of resolution
retest-video resolution="1280x720":
    rm -f wallpapers/bg.mp4
    just test-video {{resolution}}

# Remove local build and test wallpaper artifacts
clean:
    rm -rf wallpapers
