const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Copies `text` to the system clipboard via the OSC 52 terminal escape
/// sequence, which most modern terminals (and SSH sessions) support without
/// needing a native clipboard library or X11/Wayland access.
///
/// Writes to `/dev/tty` rather than stdout: in `--pick` mode the TUI itself
/// renders over `/dev/tty` precisely so stdout stays clean for
/// `$(treeq --pick f.json)` / `| tmux load-buffer -`, but the escape
/// sequence was still going to stdout, corrupting the very output pick mode
/// exists to keep clean (issue #124). Falling back to stdout when
/// `/dev/tty` can't be opened keeps this working outside pick mode and on
/// platforms without a `/dev/tty` (there stdout was already the terminal).
pub fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let encoded = base64_encode(text.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    match std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        Ok(mut tty) => {
            write!(tty, "{sequence}")?;
            tty.flush()
        }
        Err(_) => {
            let mut stdout = std::io::stdout();
            write!(stdout, "{sequence}")?;
            stdout.flush()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_to_clipboard_succeeds_even_without_a_controlling_terminal() {
        // No `/dev/tty` in a test harness process (or none reachable) must
        // still fall back to stdout rather than erroring outright -- the
        // `/dev/tty`-first change for issue #124 must not regress the
        // no-tty case (e.g. output piped to a file with no session tty).
        assert!(copy_to_clipboard("hello").is_ok());
    }

    #[test]
    fn encodes_rfc4648_test_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
