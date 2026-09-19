//! Copying to the user's clipboard through the terminal, which works over SSH.

/// The bytes that ask a terminal to put `text` on the system clipboard
/// (OSC 52). Inside tmux the request is wrapped so tmux passes it on; pass
/// whether the `TMUX` environment variable is set. Terminals that do not
/// support the request, or have it turned off, ignore it.
pub fn clipboard(text: &str, tmux: bool) -> Vec<u8> {
    let mut request = b"\x1b]52;c;".to_vec();
    request.extend(base64(text.as_bytes()));
    request.extend_from_slice(b"\x1b\\");
    if !tmux {
        return request;
    }
    // Passthrough doubles every escape byte inside the wrapper.
    let mut wrapped = b"\x1bPtmux;".to_vec();
    for byte in request {
        if byte == 0x1b {
            wrapped.push(0x1b);
        }
        wrapped.push(byte);
    }
    wrapped.extend_from_slice(b"\x1b\\");
    wrapped
}

fn base64(bytes: &[u8]) -> Vec<u8> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let bits = u32::from(group[0]) << 16 | u32::from(group[1]) << 8 | u32::from(group[2]);
        for position in 0..4 {
            out.push(if position <= chunk.len() {
                ALPHABET[(bits >> (18 - 6 * position) & 63) as usize]
            } else {
                b'='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_carry_padded_base64_and_tmux_wraps_them() {
        assert_eq!(clipboard("hi", false), b"\x1b]52;c;aGk=\x1b\\");
        assert_eq!(clipboard("hey", false), b"\x1b]52;c;aGV5\x1b\\");
        assert_eq!(
            clipboard("h", true),
            b"\x1bPtmux;\x1b\x1b]52;c;aA==\x1b\x1b\\\x1b\\"
        );
    }
}
