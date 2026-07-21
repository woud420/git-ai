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

#[derive(Clone, Debug)]
pub(super) struct Token {
    pub(super) lexeme: String,
    pub(super) start: usize,
    pub(super) end: usize,
}

impl PartialEq for Token {
    fn eq(&self, other: &Self) -> bool {
        self.lexeme == other.lexeme
    }
}

impl Eq for Token {}

impl Hash for Token {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lexeme.hash(state);
    }
}

impl PartialOrd for Token {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Token {
    fn cmp(&self, other: &Self) -> Ordering {
        self.lexeme.cmp(&other.lexeme)
    }
}

/// Check if a character is a single-character operator or delimiter
fn is_operator_or_delimiter(ch: char) -> bool {
    matches!(
        ch,
        '+' | '-'
            | '*'
            | '/'
            | '%'
            | '='
            | '<'
            | '>'
            | '!'
            | '&'
            | '|'
            | '^'
            | '~'
            | '?'
            | '@'
            | ';'
            | ','
            | '.'
            | ':'
            | '('
            | ')'
            | '{'
            | '}'
            | '['
            | ']'
    )
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
pub(super) fn tokenize_non_whitespace(content: &str, range: (usize, usize)) -> Vec<Token> {
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

        // Peek ahead for multi-character operators
        let peek = content[i + ch_len..end].chars().next();
        if let Some(op) = try_match_multi_char_op(ch, peek) {
            let op_len = op.len();
            tokens.push(Token {
                lexeme: op.to_string(),
                start: i,
                end: i + op_len,
            });
            i += op_len;
            continue;
        }

        // String literals (single token, handle escape sequences)
        if ch == '"' || ch == '\'' || ch == '`' {
            let quote_char = ch;
            let mut lexeme = String::new();
            lexeme.push(ch);
            let token_start = i;
            i += ch_len;

            let mut escaped = false;
            while i < end {
                let str_ch = match content[i..].chars().next() {
                    Some(c) => c,
                    None => break,
                };
                let str_ch_len = str_ch.len_utf8();
                lexeme.push(str_ch);

                if escaped {
                    escaped = false;
                } else if str_ch == '\\' {
                    escaped = true;
                } else if str_ch == quote_char {
                    i += str_ch_len;
                    break;
                }

                i += str_ch_len;
            }

            tokens.push(Token {
                lexeme,
                start: token_start,
                end: i,
            });
            continue;
        }

        // Numbers (including hex, octal, binary, floats, scientific notation)
        if ch.is_ascii_digit()
            || (ch == '.'
                && i + ch_len < end
                && content[i + ch_len..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit()))
        {
            let mut lexeme = String::new();
            let token_start = i;

            // Handle hex (0x), octal (0o), binary (0b) prefixes
            if ch == '0' && i + 1 < end {
                let next_ch = content[i + 1..].chars().next().unwrap();
                if next_ch == 'x'
                    || next_ch == 'X'
                    || next_ch == 'o'
                    || next_ch == 'O'
                    || next_ch == 'b'
                    || next_ch == 'B'
                {
                    lexeme.push(ch);
                    lexeme.push(next_ch);
                    i += 1 + next_ch.len_utf8();

                    // Consume hex/octal/binary digits
                    while i < end {
                        let digit_ch = match content[i..].chars().next() {
                            Some(c) => c,
                            None => break,
                        };
                        if digit_ch.is_ascii_alphanumeric() || digit_ch == '_' {
                            lexeme.push(digit_ch);
                            i += digit_ch.len_utf8();
                        } else {
                            break;
                        }
                    }

                    tokens.push(Token {
                        lexeme,
                        start: token_start,
                        end: i,
                    });
                    continue;
                }
            }

            // Regular decimal number
            while i < end {
                let num_ch = match content[i..].chars().next() {
                    Some(c) => c,
                    None => break,
                };
                let num_ch_len = num_ch.len_utf8();

                if num_ch.is_ascii_digit() || num_ch == '.' || num_ch == '_' {
                    lexeme.push(num_ch);
                    i += num_ch_len;
                } else if (num_ch == 'e' || num_ch == 'E') && i + num_ch_len < end {
                    // Scientific notation
                    lexeme.push(num_ch);
                    i += num_ch_len;
                    // Handle optional +/- after e
                    if i < end {
                        let sign_ch = content[i..].chars().next().unwrap();
                        if sign_ch == '+' || sign_ch == '-' {
                            lexeme.push(sign_ch);
                            i += sign_ch.len_utf8();
                        }
                    }
                } else {
                    break;
                }
            }

            tokens.push(Token {
                lexeme,
                start: token_start,
                end: i,
            });
            continue;
        }

        // Identifiers (alphanumeric + underscore, start with letter or underscore)
        if ch.is_alphabetic() || ch == '_' {
            let mut lexeme = String::new();
            let token_start = i;

            while i < end {
                let id_ch = match content[i..].chars().next() {
                    Some(c) => c,
                    None => break,
                };
                let id_ch_len = id_ch.len_utf8();

                if id_ch.is_alphanumeric() || id_ch == '_' {
                    lexeme.push(id_ch);
                    i += id_ch_len;
                } else {
                    break;
                }
            }

            tokens.push(Token {
                lexeme,
                start: token_start,
                end: i,
            });
            continue;
        }

        // Single-character operators and delimiters
        if is_operator_or_delimiter(ch) {
            tokens.push(Token {
                lexeme: ch.to_string(),
                start: i,
                end: i + ch_len,
            });
            i += ch_len;
            continue;
        }

        // Fallback: unknown characters as single tokens
        tokens.push(Token {
            lexeme: ch.to_string(),
            start: i,
            end: i + ch_len,
        });
        i += ch_len;
    }

    tokens
}
