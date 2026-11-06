//! Serial console output carries ANSI escape sequences (colors, cursor
//! movement); they are stripped before pattern matching.

/// Strip ANSI escape codes from a string.
///
/// Handles:
/// - CSI sequences: \x1b[...
/// - OSC sequences: \x1b]... (terminated by BEL or ST)
/// - DCS sequences: \x1bP... (terminated by ST)
/// - Single-character sequences: \x1b followed by single char
/// - Control characters: BEL, NUL, SI, SO
pub fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                // CSI sequences: \x1b[...
                Some(&'[') => {
                    chars.next();
                    // Parameter and intermediate bytes run until the final
                    // byte, 0x40-0x7E (of which `[\]^_` are not accepted here).
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic()
                            || next == '@'
                            || next == '`'
                            || (next as u8 >= 0x70 && next as u8 <= 0x7E)
                        {
                            break;
                        }
                    }
                }
                // OSC sequences: \x1b]... (terminated by BEL \x07 or ST \x1b\\)
                Some(&']') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x07' {
                            break;
                        }
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // DCS sequences: \x1b P... (terminated by ST)
                Some(&'P') => {
                    chars.next();
                    while let Some(next) = chars.next() {
                        if next == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Single-character sequences: \x1b followed by single char
                Some(&c2)
                    if c2.is_ascii_alphabetic()
                        || c2 == '>'
                        || c2 == '='
                        || c2 == '('
                        || c2 == ')' =>
                {
                    chars.next();
                    // \x1b( and \x1b) select a charset, named by one more char.
                    if c2 == '(' || c2 == ')' {
                        chars.next();
                    }
                }
                _ => {
                    // Unknown escape, skip just the ESC
                }
            }
        } else if c == '\x07' || c == '\x00' || c == '\x0f' || c == '\x0e' {
            // Skip BEL, NUL, SI, SO control characters
        } else {
            result.push(c);
        }
    }
    result
}
