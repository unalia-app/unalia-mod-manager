//! Parseur minimal du format KeyValues de Valve (texte), utilisé par
//! `libraryfolders.vdf` et les `appmanifest_*.acf`.
//!
//! Gère : chaînes entre guillemets avec échappements (`\\`, `\"`, `\n`, `\t`),
//! jetons sans guillemets, commentaires `//`, conditions `[$WIN32]` (ignorées),
//! BOM UTF-8 et fins de ligne CRLF.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Str(String),
    Obj(Obj),
}

/// Liste ordonnée de paires clé/valeur. Les clés sont comparées sans tenir
/// compte de la casse, comme le fait Steam.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Obj(Vec<(String, Value)>);

impl Obj {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Obj(_) => None,
        }
    }

    pub fn obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key)? {
            Value::Obj(o) => Some(o),
            Value::Str(_) => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Analyse un document KeyValues complet. Le résultat contient les paires de
/// premier niveau (en général une seule, ex. `"libraryfolders" { ... }`).
pub fn parse(input: &str) -> Result<Obj, ParseError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut lexer = Lexer {
        chars: input.chars().peekable(),
        line: 1,
    };
    let obj = parse_pairs(&mut lexer, false)?;
    Ok(obj)
}

fn parse_pairs(lexer: &mut Lexer<'_>, nested: bool) -> Result<Obj, ParseError> {
    let mut pairs = Vec::new();
    loop {
        let key = match lexer.next_token()? {
            None if nested => return Err(lexer.error("missing closing brace")),
            None => return Ok(Obj(pairs)),
            Some(Token::Close) if nested => return Ok(Obj(pairs)),
            Some(Token::Close) => return Err(lexer.error("unexpected closing brace")),
            Some(Token::Open) => return Err(lexer.error("expected a key before a brace")),
            Some(Token::Str(s)) => s,
        };
        let value = match lexer.next_token()? {
            Some(Token::Str(s)) => Value::Str(s),
            Some(Token::Open) => Value::Obj(parse_pairs(lexer, true)?),
            Some(Token::Close) | None => return Err(lexer.error("missing value after a key")),
        };
        pairs.push((key, value));
    }
}

enum Token {
    Open,
    Close,
    Str(String),
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
}

impl Lexer<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        ParseError {
            line: self.line,
            message,
        }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.next();
        if c == Some('\n') {
            self.line += 1;
        }
        c
    }

    fn next_token(&mut self) -> Result<Option<Token>, ParseError> {
        loop {
            self.skip_whitespace_and_comments();
            match self.chars.peek() {
                None => return Ok(None),
                Some('{') => {
                    self.bump();
                    return Ok(Some(Token::Open));
                }
                Some('}') => {
                    self.bump();
                    return Ok(Some(Token::Close));
                }
                Some('"') => {
                    self.bump();
                    return self.quoted().map(|s| Some(Token::Str(s)));
                }
                Some('[') => self.skip_conditional()?,
                Some(_) => return Ok(Some(Token::Str(self.unquoted()))),
            }
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
                self.bump();
            }
            let mut ahead = self.chars.clone();
            if ahead.next() == Some('/') && ahead.next() == Some('/') {
                while self.chars.peek().is_some_and(|&c| c != '\n') {
                    self.bump();
                }
            } else {
                return;
            }
        }
    }

    fn skip_conditional(&mut self) -> Result<(), ParseError> {
        while let Some(c) = self.bump() {
            if c == ']' {
                return Ok(());
            }
        }
        Err(self.error("unclosed condition"))
    }

    fn quoted(&mut self) -> Result<String, ParseError> {
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("missing closing quote")),
                Some('"') => return Ok(out),
                Some('\\') => match self.bump() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err(self.error("missing closing quote")),
                },
                Some(c) => out.push(c),
            }
        }
    }

    fn unquoted(&mut self) -> String {
        let mut out = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                break;
            }
            out.push(c);
            self.bump();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_objects() {
        let doc = parse(
            r#"
            "AppState"
            {
                "appid"     "1030300"
                "UserConfig" { "language" "french" }
            }
            "#,
        )
        .unwrap();
        let state = doc.obj("AppState").unwrap();
        assert_eq!(state.str("appid"), Some("1030300"));
        assert_eq!(
            state.obj("UserConfig").unwrap().str("language"),
            Some("french")
        );
    }

    #[test]
    fn keys_are_case_insensitive() {
        let doc = parse(r#""AppState" { "AppID" "42" }"#).unwrap();
        assert_eq!(doc.obj("appstate").unwrap().str("appid"), Some("42"));
    }

    #[test]
    fn unescapes_windows_paths() {
        let doc = parse(r#""path" "D:\\SteamLibrary\\Jeux \"bonus\"""#).unwrap();
        assert_eq!(doc.str("path"), Some(r#"D:\SteamLibrary\Jeux "bonus""#));
    }

    #[test]
    fn keeps_unknown_escapes_verbatim() {
        let doc = parse(r#""path" "C:\Games""#).unwrap();
        assert_eq!(doc.str("path"), Some(r"C:\Games"));
    }

    #[test]
    fn handles_bom_crlf_comments_and_conditionals() {
        let doc = parse(
            "\u{feff}// en-tête\r\n\"root\"\r\n{\r\n\t\"a\" \"1\" [$WIN32]\r\n\t// note\r\n\tb 2\r\n}\r\n",
        )
        .unwrap();
        let root = doc.obj("root").unwrap();
        assert_eq!(root.str("a"), Some("1"));
        assert_eq!(root.str("b"), Some("2"));
    }

    #[test]
    fn single_slash_is_part_of_a_token() {
        let doc = parse("\"k\" a/b").unwrap();
        assert_eq!(doc.str("k"), Some("a/b"));
    }

    #[test]
    fn reports_line_of_errors() {
        let err = parse("\"root\"\n{\n\"a\" \"1\"\n").unwrap_err();
        assert_eq!(err.line, 4);

        assert!(parse("\"a\" }").is_err());
        assert!(parse("\"a\"").is_err());
        assert!(parse("\"a").is_err());
        assert!(parse("{ }").is_err());
    }

    #[test]
    fn str_and_obj_check_the_value_kind() {
        let doc = parse(r#""a" "1" "b" {}"#).unwrap();
        assert_eq!(doc.obj("a"), None);
        assert_eq!(doc.str("b"), None);
        assert_eq!(doc.iter().count(), 2);
    }
}
