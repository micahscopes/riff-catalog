//! Hand-rolled Yul lexer. Spelling-preserving for numbers; strings and hex
//! strings are unescaped to bytes here so the parser can build canonical
//! literals.

use crate::error::YulParseError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Ident(String),
    Number(String),
    Str(Vec<u8>),
    HexStr(Vec<u8>),
    LBrace,
    RBrace,
    LParen,
    RParen,
    Comma,
    Colon,
    Assign, // :=
    Arrow,  // ->
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pos {
    pub line: u32,
    pub col: u32,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    line: u32,
    col: u32,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.bytes.get(self.at + 1).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        if byte == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(byte)
    }

    fn pos(&self) -> Pos {
        Pos {
            line: self.line,
            col: self.col,
        }
    }

    fn error(&self, message: impl Into<String>) -> YulParseError {
        YulParseError::new(self.line, self.col, message)
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' || byte == b'.'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte == b'.'
}

pub(crate) fn lex(src: &str) -> Result<Vec<(Token, Pos)>, YulParseError> {
    let mut cursor = Cursor {
        bytes: src.as_bytes(),
        at: 0,
        line: 1,
        col: 1,
    };
    let mut tokens = Vec::new();

    while let Some(byte) = cursor.peek() {
        let pos = cursor.pos();
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => {
                cursor.bump();
            }
            b'/' if cursor.peek2() == Some(b'/') => {
                while let Some(byte) = cursor.peek() {
                    if byte == b'\n' {
                        break;
                    }
                    cursor.bump();
                }
            }
            b'/' if cursor.peek2() == Some(b'*') => {
                cursor.bump();
                cursor.bump();
                loop {
                    match cursor.peek() {
                        None => return Err(cursor.error("unterminated block comment")),
                        Some(b'*') if cursor.peek2() == Some(b'/') => {
                            cursor.bump();
                            cursor.bump();
                            break;
                        }
                        _ => {
                            cursor.bump();
                        }
                    }
                }
            }
            b'{' => {
                cursor.bump();
                tokens.push((Token::LBrace, pos));
            }
            b'}' => {
                cursor.bump();
                tokens.push((Token::RBrace, pos));
            }
            b'(' => {
                cursor.bump();
                tokens.push((Token::LParen, pos));
            }
            b')' => {
                cursor.bump();
                tokens.push((Token::RParen, pos));
            }
            b',' => {
                cursor.bump();
                tokens.push((Token::Comma, pos));
            }
            b':' => {
                cursor.bump();
                if cursor.peek() == Some(b'=') {
                    cursor.bump();
                    tokens.push((Token::Assign, pos));
                } else {
                    tokens.push((Token::Colon, pos));
                }
            }
            b'-' => {
                cursor.bump();
                if cursor.peek() == Some(b'>') {
                    cursor.bump();
                    tokens.push((Token::Arrow, pos));
                } else {
                    return Err(cursor.error("expected `->`"));
                }
            }
            b'"' | b'\'' => {
                let bytes = lex_string(&mut cursor)?;
                tokens.push((Token::Str(bytes), pos));
            }
            b'0'..=b'9' => {
                let spelling = lex_number(&mut cursor);
                tokens.push((Token::Number(spelling), pos));
            }
            byte if is_ident_start(byte) => {
                let mut ident = String::new();
                while let Some(byte) = cursor.peek() {
                    if is_ident_continue(byte) {
                        ident.push(byte as char);
                        cursor.bump();
                    } else {
                        break;
                    }
                }
                // hex string: `hex` immediately followed by a quote
                if ident == "hex" && matches!(cursor.peek(), Some(b'"') | Some(b'\'')) {
                    let bytes = lex_hex_string(&mut cursor)?;
                    tokens.push((Token::HexStr(bytes), pos));
                } else {
                    tokens.push((Token::Ident(ident), pos));
                }
            }
            other => {
                return Err(cursor.error(format!("unexpected character `{}`", other as char)));
            }
        }
    }
    Ok(tokens)
}

fn lex_number(cursor: &mut Cursor<'_>) -> String {
    let mut spelling = String::new();
    if cursor.peek() == Some(b'0') && matches!(cursor.peek2(), Some(b'x') | Some(b'X')) {
        spelling.push(cursor.bump().unwrap() as char);
        spelling.push(cursor.bump().unwrap() as char);
        while let Some(byte) = cursor.peek() {
            if byte.is_ascii_hexdigit() {
                spelling.push(byte as char);
                cursor.bump();
            } else {
                break;
            }
        }
    } else {
        while let Some(byte) = cursor.peek() {
            if byte.is_ascii_digit() {
                spelling.push(byte as char);
                cursor.bump();
            } else {
                break;
            }
        }
    }
    spelling
}

fn lex_string(cursor: &mut Cursor<'_>) -> Result<Vec<u8>, YulParseError> {
    let quote = cursor.bump().expect("caller saw a quote");
    let mut bytes = Vec::new();
    loop {
        match cursor.peek() {
            None => return Err(cursor.error("unterminated string literal")),
            Some(byte) if byte == quote => {
                cursor.bump();
                return Ok(bytes);
            }
            Some(b'\\') => {
                cursor.bump();
                let escape = cursor
                    .bump()
                    .ok_or_else(|| cursor.error("unterminated escape"))?;
                match escape {
                    b'\\' => bytes.push(b'\\'),
                    b'"' => bytes.push(b'"'),
                    b'\'' => bytes.push(b'\''),
                    b'n' => bytes.push(b'\n'),
                    b'r' => bytes.push(b'\r'),
                    b't' => bytes.push(b'\t'),
                    b'b' => bytes.push(0x08),
                    b'f' => bytes.push(0x0c),
                    b'v' => bytes.push(0x0b),
                    b'0' => bytes.push(0),
                    b'x' => {
                        let hi = hex_digit(cursor)?;
                        let lo = hex_digit(cursor)?;
                        bytes.push((hi << 4) | lo);
                    }
                    b'u' => {
                        let mut code: u32 = 0;
                        for _ in 0..4 {
                            code = (code << 4) | u32::from(hex_digit(cursor)?);
                        }
                        let ch = char::from_u32(code)
                            .ok_or_else(|| cursor.error("invalid \\u escape"))?;
                        let mut buffer = [0u8; 4];
                        bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
                    }
                    other => {
                        return Err(cursor.error(format!("unknown escape `\\{}`", other as char)));
                    }
                }
            }
            Some(b'\n') => return Err(cursor.error("newline in string literal")),
            Some(byte) => {
                bytes.push(byte);
                cursor.bump();
            }
        }
    }
}

fn lex_hex_string(cursor: &mut Cursor<'_>) -> Result<Vec<u8>, YulParseError> {
    let quote = cursor.bump().expect("caller saw a quote");
    let mut nibbles = Vec::new();
    loop {
        match cursor.peek() {
            None => return Err(cursor.error("unterminated hex string")),
            Some(byte) if byte == quote => {
                cursor.bump();
                break;
            }
            Some(b'_') => {
                cursor.bump();
            }
            Some(byte) if byte.is_ascii_hexdigit() => {
                nibbles.push(hex_value(byte));
                cursor.bump();
            }
            Some(other) => {
                return Err(
                    cursor.error(format!("invalid hex string character `{}`", other as char))
                );
            }
        }
    }
    if nibbles.len() % 2 != 0 {
        return Err(cursor.error("odd number of hex digits in hex string"));
    }
    Ok(nibbles
        .chunks_exact(2)
        .map(|pair| (pair[0] << 4) | pair[1])
        .collect())
}

fn hex_digit(cursor: &mut Cursor<'_>) -> Result<u8, YulParseError> {
    let byte = cursor
        .bump()
        .ok_or_else(|| cursor.error("unterminated hex escape"))?;
    if byte.is_ascii_hexdigit() {
        Ok(hex_value(byte))
    } else {
        Err(cursor.error("invalid hex digit"))
    }
}

fn hex_value(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("checked by caller"),
    }
}
