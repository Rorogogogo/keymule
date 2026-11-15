//! Find a secret in output in the forms it usually turns up in: as it is,
//! URL-encoded, and base64 (on its own or inside a longer base64 string,
//! such as a Basic auth header). Used for API responses and for the output
//! of commands `keymule exec` hands a credential to.

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE};

pub const REDACTED: &str = "[redacted by keymule]";

/// Shorter forms than this aren't scrubbed: they'd match by chance.
const MIN_FORM: usize = 8;

/// Every form of `secret` to scrub, longest first.
pub fn forms(secret: &str) -> Vec<String> {
    if secret.len() < 4 {
        return vec![];
    }
    let mut out = vec![secret.to_string()];
    out.push(percent_encode(secret, false));
    out.push(percent_encode(secret, true));
    for engine in [STANDARD, URL_SAFE] {
        // base64 groups bytes in threes, so inside a longer string the
        // secret's encoding depends on where it starts. For each of the three
        // offsets, keep the characters that come from the secret alone.
        for k in 0..3 {
            let mut bytes = vec![0u8; k];
            bytes.extend_from_slice(secret.as_bytes());
            let encoded = engine.encode(&bytes);
            let full = bytes.len() / 3 * 4;
            let start = if k == 0 { 0 } else { 4 };
            if full > start {
                out.push(encoded[start..full].to_string());
            }
            if k == 0 {
                // The whole thing, padding included, on its own.
                out.push(encoded.trim_end_matches('=').to_string());
            }
        }
    }
    out.retain(|f| f.len() >= MIN_FORM || f == secret);
    out.sort_by_key(|f| std::cmp::Reverse(f.len()));
    out.dedup();
    out
}

fn percent_encode(s: &str, lower: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else if lower {
            out.push_str(&format!("%{b:02x}"));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `text` with every form of `secret` replaced.
pub fn scrub(text: &str, secret: &str) -> String {
    let mut out = text.to_string();
    for f in forms(secret) {
        out = out.replace(&f, REDACTED);
    }
    out
}

fn replace_bytes(hay: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() || hay.len() < needle.len() {
        return hay.to_vec();
    }
    let mut out = Vec::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if hay[i..].starts_with(needle) {
            out.extend_from_slice(with);
            i += needle.len();
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

/// Scrubs a stream that arrives in chunks: it holds back just enough of the
/// end that a secret split across two chunks is still caught.
pub struct Scrubber {
    forms: Vec<Vec<u8>>,
    keep: usize,
    buf: Vec<u8>,
}

impl Scrubber {
    pub fn new(secret: &str) -> Self {
        let forms: Vec<Vec<u8>> = forms(secret).into_iter().map(String::into_bytes).collect();
        let keep = forms
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1);
        Self {
            forms,
            keep,
            buf: Vec::new(),
        }
    }

    fn scrub_buf(&mut self) {
        for f in &self.forms {
            self.buf = replace_bytes(&self.buf, f, REDACTED.as_bytes());
        }
    }

    /// Takes the next chunk, returns what's safe to write now.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.buf.extend_from_slice(chunk);
        self.scrub_buf();
        let cut = self.buf.len().saturating_sub(self.keep);
        self.buf.drain(..cut).collect()
    }

    /// The rest, at the end of the stream.
    pub fn finish(&mut self) -> Vec<u8> {
        self.scrub_buf();
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn catches_raw_url_encoded_and_base64_forms() {
        let s = "p@ss/w0rd+long-value";
        let text = format!(
            "raw {s} url {} b64 {} basic {}",
            percent_encode(s, false),
            STANDARD.encode(s),
            STANDARD.encode(format!("me:{s}")),
        );
        let out = scrub(&text, s);
        assert!(!out.contains(s), "{out}");
        assert!(!out.contains(&percent_encode(s, false)), "{out}");
        assert!(!out.contains(&STANDARD.encode(s)), "{out}");
        // In a Basic header only the secret's own part goes; the rest stays.
        assert!(out.contains("basic "), "{out}");
        for k in 0..3 {
            let prefix = "x".repeat(k + 3);
            let enc = STANDARD.encode(format!("{prefix}{s}"));
            let core = &enc[8..enc.len() - 4];
            assert!(!scrub(&enc, s).contains(core), "offset {k}: {enc}");
        }
    }

    #[test]
    fn a_secret_split_across_chunks_is_still_caught() {
        let text = format!("token: {TOKEN}\nb64: {}\ndone\n", STANDARD.encode(TOKEN));
        for size in [1, 3, 7, 16, 41, 1000] {
            let mut s = Scrubber::new(TOKEN);
            let mut out = Vec::new();
            for chunk in text.as_bytes().chunks(size) {
                out.extend(s.push(chunk));
            }
            out.extend(s.finish());
            let out = String::from_utf8(out).unwrap();
            assert!(
                !out.contains(TOKEN) && !out.contains(&STANDARD.encode(TOKEN)),
                "{size}: {out}"
            );
            assert!(
                out.starts_with("token: [redacted by keymule]") && out.ends_with("done\n"),
                "{out}"
            );
        }
    }

    #[test]
    fn short_secrets_are_left_alone() {
        assert!(forms("abc").is_empty());
        assert_eq!(scrub("no secret here", TOKEN), "no secret here");
    }
}
