Regenerates `../demo.gif`. No headless-browser dependency (unlike `vhs`,
which depends on a chromedp pipeline that didn't complete headlessly in the
environment this was recorded in) -- records a real pty session as an
asciicast, then rasterizes it with `agg`.

```sh
cargo build --release
python3 assets/demo/record.py assets/demo/demo.cast
agg --font-family "JetBrainsMono Nerd Font Mono" --font-size 18 \
    --theme "1e1e2e,cdd6f4,45475a,f38ba8,a6e3a1,f9e2af,89b4fa,f5c2e7,94e2d5,bac2de,585b70,f38ba8,a6e3a1,f9e2af,89b4fa,f5c2e7,94e2d5,a6adc8" \
    assets/demo/demo.cast assets/demo.gif
rm assets/demo/demo.cast   # not committed -- regenerate instead of diffing it
```

`sample.json` is the fixture the recording drives; edit the keystroke
sequence at the bottom of `record.py` to change what the GIF shows.
