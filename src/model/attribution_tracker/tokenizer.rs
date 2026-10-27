//! Line metadata collection and token-based tokenization.
//!
//! `collect_line_metadata` is used by the diff engine and move-detection
//! integration inside the tracker.  `tokenize_non_whitespace` drives the
//! token-aligned diff path in the diff engine.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub(super) struct LineMetadata {
    pub(super) number: usize,
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) text: String,
}

pub(super) fn collect_line_metadata(content: &str) -> Vec<LineMetadata> {
    let mut metadata = Vec::new();
    let mut line_start = 0usize;
    let mut line_number = 1usize;

    for (idx, ch) in content.char_indices() {
        if ch == '\n' {
            let slice = &content[line_start..idx];
            let mut text = slice.to_string();
            if text.ends_with('\r') {
                text.pop();
            }
            metadata.push(LineMetadata {
                number: line_number,
                start: line_start,
                end: idx + 1,
                text,
            });
            line_start = idx + 1;
            line_number += 1;
        }
    }

    if line_start < content.len() {
        let slice = &content[line_start..content.len()];
        let mut text = slice.to_string();
        if text.ends_with('\r') {
            text.pop();
        }
        metadata.push(LineMetadata {
            number: line_number,
            start: line_start,
            end: content.len(),
            text,
        });
    }

    metadata
}

/// A token borrowed from the tokenized content: `lexeme` is always
/// `&content[start..end]`.
#[derive(Clone, Debug)]
pub(super) struct Token<'a> {
    pub(super) lexeme: &'a str,
    pub(super) start: usize,
    pub(super) end: usize,
}

impl PartialEq for Token<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.lexeme == other.lexeme
    }
}

impl Eq for Token<'_> {}

impl Hash for Token<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lexeme.hash(state);
    }
}

impl PartialOrd for Token<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Token<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.lexeme.cmp(other.lexeme)
    }
}

/// Try to match a multi-character operator by peeking ahead
fn try_match_multi_char_op(ch: char, peek: Option<char>) -> Option<&'static str> {
    match (ch, peek) {
        ('=', Some('=')) => Some("=="),
        ('!', Some('=')) => Some("!="),
        ('<', Some('=')) => Some("<="),
        ('>', Some('=')) => Some(">="),
        ('&', Some('&')) => Some("&&"),
        ('|', Some('|')) => Some("||"),
        (':', Some(':')) => Some("::"),
        ('-', Some('>')) => Some("->"),
        ('=', Some('>')) => Some("=>"),
        ('.', Some('.')) => Some(".."),
        ('+', Some('+')) => Some("++"),
        ('-', Some('-')) => Some("--"),
        ('+', Some('=')) => Some("+="),
        ('-', Some('=')) => Some("-="),
        ('*', Some('=')) => Some("*="),
        ('/', Some('=')) => Some("/="),
        ('%', Some('=')) => Some("%="),
        ('&', Some('=')) => Some("&="),
        ('|', Some('=')) => Some("|="),
        ('^', Some('=')) => Some("^="),
        ('<', Some('<')) => Some("<<"),
        ('>', Some('>')) => Some(">>"),
        _ => None,
    }
}

/// Code-optimized tokenizer that treats syntactic elements as meaningful units
pub(super) fn tokenize_non_whitespace(content: &str, range: (usize, usize)) -> Vec<Token<'_>> {
    let (start, end) = range;
    if start >= end {
        return Vec::new();
    }

    let mut tokens = Vec::new();

    let mut i = start;
    while i < end {
        let ch = match content[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        let ch_len = ch.len_utf8();

        // Skip whitespace
        if ch.is_whitespace() {
            i += ch_len;
            continue;
        }

        let token_start = i;

        // Peek ahead for multi-character operators
        let peek = content[i + ch_len..end].chars().next();
        if let Some(op) = try_match_multi_char_op(ch, peek) {
            i += op.len();
        } else if ch == '"' || ch == '\'' || ch == '`' {
            // String literals (single token, handle escape sequences)
            let quote_char = ch;
            i += ch_len;

            let mut escaped = false;
            while i < end {
                let str_ch = match content[i..].chars().next() {
                    Some(c) => c,
                    None => break,
                };
                i += str_ch.len_utf8();

                if escaped {
                    escaped = false;
                } else if str_ch == '\\' {
                    escaped = true;
                } else if str_ch == quote_char {
                    break;
                }
            }
        } else if ch.is_ascii_digit()
            || (ch == '.'
                && i + ch_len < end
                && content[i + ch_len..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()))
        {
            // Numbers (including hex, octal, binary, floats, scientific notation)
            let radix_prefix = ch == '0'
                && i + 1 < end
                && matches!(
                    content[i + 1..].chars().next(),
                    Some('x' | 'X' | 'o' | 'O' | 'b' | 'B')
                );

            if radix_prefix {
                // Consume the `0x`/`0o`/`0b` prefix, then hex/octal/binary digits
                i += 2;
                while i < end {
                    match content[i..].chars().next() {
                        Some(c) if c.is_ascii_alphanumeric() || c == '_' => i += c.len_utf8(),
                        _ => break,
                    }
                }
            } else {
                // Regular decimal number
                while i < end {
                    let num_ch = match content[i..].chars().next() {
                        Some(c) => c,
                        None => break,
                    };
                    let num_ch_len = num_ch.len_utf8();

                    if num_ch.is_ascii_digit() || num_ch == '.' || num_ch == '_' {
                        i += num_ch_len;
                    } else if (num_ch == 'e' || num_ch == 'E') && i + num_ch_len < end {
                        // Scientific notation
                        i += num_ch_len;
                        // Handle optional +/- after e
                        if i < end {
                            let sign_ch = content[i..].chars().next().unwrap();
                            if sign_ch == '+' || sign_ch == '-' {
                                i += sign_ch.len_utf8();
                            }
                        }
                    } else {
                        break;
                    }
                }
            }
        } else if ch.is_alphabetic() || ch == '_' {
            // Identifiers (alphanumeric + underscore, start with letter or underscore)
            while i < end {
                match content[i..].chars().next() {
                    Some(c) if c.is_alphanumeric() || c == '_' => i += c.len_utf8(),
                    _ => break,
                }
            }
        } else {
            // Single-character operators, delimiters, and any other character
            i += ch_len;
        }

        tokens.push(Token {
            lexeme: &content[token_start..i],
            start: token_start,
            end: i,
        });
    }

    tokens
}
