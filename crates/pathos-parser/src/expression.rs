//! Expression parser for `{if: condition}` and `Link.enabled_if`.
//!
//! Recursive-descent parser that produces `pathos_core::Expression` AST nodes.
//! Handles: literals, state references (`$path`), operators, function calls.

use pathos_core::{Expression, Value};

/// Parse an expression string into an `Expression` AST.
///
/// Returns `Err(message)` on parse failure.
pub fn parse_expression(input: &str) -> Result<Expression, String> {
    let tokens = tokenize(input);
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_or()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("unexpected token: {:?}", parser.current()));
    }
    Ok(expr)
}

// ── Tokenizer ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// `$identifier` state variable reference
    StateRef(String),
    /// Integer literal
    Int(i64),
    /// Float literal
    Float(f64),
    /// String literal (without surrounding quotes)
    StringLit(String),
    /// Identifier (function names, true, false, null)
    Ident(String),
    /// `(` open paren
    LParen,
    /// `)` close paren
    RParen,
    /// `,` comma
    Comma,
    /// `!` unary not
    Bang,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `<=`
    LtEq,
    /// `>=`
    GtEq,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        // Skip whitespace
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        // State reference: $identifier or $identifier.path...
        if ch == '$' {
            let start = i + 1;
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            if name.is_empty() {
                // Lone $ is an error — skip it
                continue;
            }
            tokens.push(Token::StateRef(name));
            continue;
        }
        // String literal
        if ch == '"' || ch == '\'' {
            let quote = ch;
            let start = i + 1;
            i += 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 2; // skip escape
                } else {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().collect();
            if i < chars.len() {
                i += 1; // skip closing quote
            }
            tokens.push(Token::StringLit(s));
            continue;
        }
        // Number
        if ch.is_ascii_digit() || (ch == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit()) {
            let start = i;
            i += 1;
            let mut is_float = false;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                if chars[i] == '.' { is_float = true; }
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if is_float {
                if let Ok(f) = num_str.parse::<f64>() {
                    tokens.push(Token::Float(f));
                }
            } else {
                if let Ok(n) = num_str.parse::<i64>() {
                    tokens.push(Token::Int(n));
                }
            }
            continue;
        }
        // Multi-char operators
        if ch == '&' && i + 1 < chars.len() && chars[i + 1] == '&' { tokens.push(Token::AndAnd); i += 2; continue; }
        if ch == '|' && i + 1 < chars.len() && chars[i + 1] == '|' { tokens.push(Token::OrOr); i += 2; continue; }
        if ch == '=' && i + 1 < chars.len() && chars[i + 1] == '=' { tokens.push(Token::EqEq); i += 2; continue; }
        if ch == '!' && i + 1 < chars.len() && chars[i + 1] == '=' { tokens.push(Token::NotEq); i += 2; continue; }
        if ch == '<' && i + 1 < chars.len() && chars[i + 1] == '=' { tokens.push(Token::LtEq); i += 2; continue; }
        if ch == '>' && i + 1 < chars.len() && chars[i + 1] == '=' { tokens.push(Token::GtEq); i += 2; continue; }
        // Single-char tokens
        match ch {
            '(' => { tokens.push(Token::LParen); i += 1; continue; }
            ')' => { tokens.push(Token::RParen); i += 1; continue; }
            ',' => { tokens.push(Token::Comma); i += 1; continue; }
            '!' => { tokens.push(Token::Bang); i += 1; continue; }
            '<' => { tokens.push(Token::Lt); i += 1; continue; }
            '>' => { tokens.push(Token::Gt); i += 1; continue; }
            '+' => { tokens.push(Token::Plus); i += 1; continue; }
            '-' => { tokens.push(Token::Minus); i += 1; continue; }
            '*' => { tokens.push(Token::Star); i += 1; continue; }
            '/' => { tokens.push(Token::Slash); i += 1; continue; }
            _ => {
                // Identifier
                if ch.is_alphabetic() || ch == '_' {
                    let start = i;
                    i += 1;
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    let ident: String = chars[start..i].iter().collect();
                    tokens.push(Token::Ident(ident));
                    continue;
                }
                i += 1; // skip unknown char
            }
        }
    }
    tokens
}

// ── Recursive-descent parser ─────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn expect(&mut self, msg: &str) -> Result<&Token, String> {
        self.peek().ok_or_else(|| format!("expected {msg}, got end of input"))
    }

    // ── Precedence levels ──────────────────────────────────────────

    /// `||` (lowest precedence)
    fn parse_or(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::OrOr) {
            self.advance();
            let right = self.parse_and()?;
            left = Expression::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `&&`
    fn parse_and(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_equality()?;
        while self.peek() == Some(&Token::AndAnd) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expression::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `==` `!=`
    fn parse_equality(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_comparison()?;
        loop {
            match self.peek() {
                Some(&Token::EqEq) => {
                    self.advance();
                    let right = self.parse_comparison()?;
                    left = Expression::Eq(Box::new(left), Box::new(right));
                }
                Some(&Token::NotEq) => {
                    self.advance();
                    let right = self.parse_comparison()?;
                    left = Expression::NotEq(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// `<` `>` `<=` `>=`
    fn parse_comparison(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_additive()?;
        loop {
            match self.peek() {
                Some(&Token::Lt) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = Expression::Lt(Box::new(left), Box::new(right));
                }
                Some(&Token::Gt) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = Expression::Gt(Box::new(left), Box::new(right));
                }
                Some(&Token::LtEq) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = Expression::Lte(Box::new(left), Box::new(right));
                }
                Some(&Token::GtEq) => {
                    self.advance();
                    let right = self.parse_additive()?;
                    left = Expression::Gte(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// `+` `-`
    fn parse_additive(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek() {
                Some(&Token::Plus) => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = Expression::Add(Box::new(left), Box::new(right));
                }
                Some(&Token::Minus) => {
                    self.advance();
                    let right = self.parse_multiplicative()?;
                    left = Expression::Sub(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// `*` `/`
    fn parse_multiplicative(&mut self) -> Result<Expression, String> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(&Token::Star) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expression::Mul(Box::new(left), Box::new(right));
                }
                Some(&Token::Slash) => {
                    self.advance();
                    let right = self.parse_unary()?;
                    left = Expression::Div(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// `!` (prefix)
    fn parse_unary(&mut self) -> Result<Expression, String> {
        if self.peek() == Some(&Token::Bang) {
            self.advance();
            let inner = self.parse_unary()?;
            return Ok(Expression::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    /// Literals, state refs, function calls, parenthesized expressions
    fn parse_primary(&mut self) -> Result<Expression, String> {
        match self.peek().cloned() {
            Some(Token::Int(n)) => { self.advance(); Ok(Expression::Literal(Value::Int(n))) }
            Some(Token::Float(f)) => { self.advance(); Ok(Expression::Literal(Value::float(f).unwrap_or(Value::Null))) }
            Some(Token::StringLit(s)) => { self.advance(); Ok(Expression::Literal(Value::String(s))) }
            Some(Token::StateRef(name)) => { self.advance(); Ok(Expression::StateVar(name)) }
            Some(Token::Ident(name)) => {
                self.advance();
                match name.as_str() {
                    "true" => Ok(Expression::Literal(Value::Bool(true))),
                    "false" => Ok(Expression::Literal(Value::Bool(false))),
                    "null" => Ok(Expression::Literal(Value::Null)),
                    _ => {
                        // Function call: fn_name(args...)
                        if self.peek() == Some(&Token::LParen) {
                            self.advance(); // skip '('
                            let mut args = Vec::new();
                            if self.peek() != Some(&Token::RParen) {
                                args.push(self.parse_or()?);
                                while self.peek() == Some(&Token::Comma) {
                                    self.advance();
                                    args.push(self.parse_or()?);
                                }
                            }
                            self.expect(")")?;
                            if self.peek() == Some(&Token::RParen) {
                                self.advance();
                            } else {
                                return Err("expected ')' after function arguments".into());
                            }
                            Ok(Expression::Call { name, args })
                        } else {
                            Err(format!("unknown identifier: {name}"))
                        }
                    }
                }
            }
            Some(Token::LParen) => {
                self.advance();
                let expr = self.parse_or()?;
                if self.peek() == Some(&Token::RParen) {
                    self.advance();
                } else {
                    return Err("expected ')'".into());
                }
                Ok(expr)
            }
            Some(tok) => Err(format!("unexpected token: {tok:?}")),
            None => Err("unexpected end of input".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── literals ────────────────────────────────────────────────────────

    #[test]
    fn parse_int_literal() {
        let e = parse_expression("42").unwrap();
        assert_eq!(e, Expression::Literal(Value::Int(42)));
    }

    #[test]
    fn parse_bool_literal_true() {
        let e = parse_expression("true").unwrap();
        assert_eq!(e, Expression::Literal(Value::Bool(true)));
    }

    #[test]
    fn parse_bool_literal_false() {
        let e = parse_expression("false").unwrap();
        assert_eq!(e, Expression::Literal(Value::Bool(false)));
    }

    #[test]
    fn parse_null_literal() {
        let e = parse_expression("null").unwrap();
        assert_eq!(e, Expression::Literal(Value::Null));
    }

    #[test]
    fn parse_string_literal() {
        let e = parse_expression(r#""hello""#).unwrap();
        assert_eq!(e, Expression::Literal(Value::String("hello".into())));
    }

    #[test]
    fn parse_float_literal() {
        let e = parse_expression("3.14").unwrap();
        assert!(matches!(e, Expression::Literal(Value::Float(_))));
    }

    // ── state refs ──────────────────────────────────────────────────────

    #[test]
    fn parse_state_ref_simple() {
        let e = parse_expression("$hp").unwrap();
        assert_eq!(e, Expression::StateVar("hp".into()));
    }

    #[test]
    fn parse_state_ref_dotted() {
        let e = parse_expression("$player.hp").unwrap();
        assert_eq!(e, Expression::StateVar("player.hp".into()));
    }

    // ── unary ───────────────────────────────────────────────────────────

    #[test]
    fn parse_not() {
        let e = parse_expression("!true").unwrap();
        assert_eq!(e, Expression::Not(Box::new(Expression::Literal(Value::Bool(true)))));
    }

    #[test]
    fn parse_double_not() {
        let e = parse_expression("!!false").unwrap();
        assert!(matches!(e, Expression::Not(_)));
    }

    // ── arithmetic ──────────────────────────────────────────────────────

    #[test]
    fn parse_addition() {
        let e = parse_expression("1 + 2").unwrap();
        assert_eq!(e, Expression::Add(
            Box::new(Expression::Literal(Value::Int(1))),
            Box::new(Expression::Literal(Value::Int(2))),
        ));
    }

    #[test]
    fn parse_multiplication_precedence() {
        // 1 + 2 * 3  →  1 + (2 * 3)
        let e = parse_expression("1 + 2 * 3").unwrap();
        assert!(matches!(e, Expression::Add(_, _)));
        if let Expression::Add(left, right) = &e {
            assert_eq!(**left, Expression::Literal(Value::Int(1)));
            assert_eq!(**right, Expression::Mul(
                Box::new(Expression::Literal(Value::Int(2))),
                Box::new(Expression::Literal(Value::Int(3))),
            ));
        }
    }

    #[test]
    fn parse_subtraction() {
        let e = parse_expression("10 - 3").unwrap();
        assert_eq!(e, Expression::Sub(
            Box::new(Expression::Literal(Value::Int(10))),
            Box::new(Expression::Literal(Value::Int(3))),
        ));
    }

    #[test]
    fn parse_mul_div_precedence() {
        let e = parse_expression("8 / 2 * 3").unwrap();
        // left-associative: (8 / 2) * 3
        assert!(matches!(e, Expression::Mul(_, _)));
    }

    // ── comparisons ─────────────────────────────────────────────────────

    #[test]
    fn parse_comparison_lt() {
        let e = parse_expression("$hp < 5").unwrap();
        assert!(matches!(e, Expression::Lt(_, _)));
    }

    #[test]
    fn parse_comparison_gte() {
        let e = parse_expression("$wisdom >= 3").unwrap();
        assert!(matches!(e, Expression::Gte(_, _)));
    }

    #[test]
    fn parse_equality() {
        let e = parse_expression("$name == \"hero\"").unwrap();
        assert!(matches!(e, Expression::Eq(_, _)));
    }

    #[test]
    fn parse_not_equals() {
        let e = parse_expression("$hp != 0").unwrap();
        assert!(matches!(e, Expression::NotEq(_, _)));
    }

    // ── logical ─────────────────────────────────────────────────────────

    #[test]
    fn parse_and() {
        let e = parse_expression("$hp > 0 && $wisdom > 2").unwrap();
        assert!(matches!(e, Expression::And(_, _)));
    }

    #[test]
    fn parse_or() {
        let e = parse_expression("$hp <= 0 || $courage > 5").unwrap();
        assert!(matches!(e, Expression::Or(_, _)));
    }

    #[test]
    fn parse_logical_precedence() {
        // a || b && c  →  a || (b && c)
        let e = parse_expression("true || false && true").unwrap();
        assert!(matches!(e, Expression::Or(_, _)));
    }

    // ── parens ──────────────────────────────────────────────────────────

    #[test]
    fn parse_parens() {
        let e = parse_expression("(1 + 2) * 3").unwrap();
        assert!(matches!(e, Expression::Mul(_, _)));
    }

    #[test]
    fn parse_nested_parens() {
        let e = parse_expression("((true))").unwrap();
        assert_eq!(e, Expression::Literal(Value::Bool(true)));
    }

    // ── function calls ──────────────────────────────────────────────────

    #[test]
    fn parse_function_call_no_args() {
        // random() is technically invalid but should parse
        let e = parse_expression("random()").unwrap();
        assert!(matches!(e, Expression::Call { name, .. } if name == "random"));
    }

    #[test]
    fn parse_function_call_with_args() {
        let e = parse_expression("random(1, 10)").unwrap();
        match e {
            Expression::Call { name, args } => {
                assert_eq!(name, "random");
                assert_eq!(args.len(), 2);
            }
            _ => panic!("expected Call"),
        }
    }

    #[test]
    fn parse_has_tag() {
        let e = parse_expression("has_tag(\"forest\")").unwrap();
        assert!(matches!(e, Expression::Call { name, .. } if name == "has_tag"));
    }

    // ── error cases ─────────────────────────────────────────────────────

    #[test]
    fn parse_error_unexpected_token() {
        assert!(parse_expression("$hp @ 5").is_err());
    }

    #[test]
    fn parse_error_unclosed_paren() {
        assert!(parse_expression("(1 + 2").is_err());
    }

    // ── complex expressions ─────────────────────────────────────────────

    #[test]
    fn parse_complex_condition() {
        // $hp > 0 && ($wisdom > 2 || $courage > 5)
        let e = parse_expression("$hp > 0 && ($wisdom > 2 || $courage > 5)").unwrap();
        assert!(matches!(e, Expression::And(_, _)));
    }

    #[test]
    fn parse_visitor_count() {
        let e = parse_expression("visited(\"dark_forest\")").unwrap();
        assert!(matches!(e, Expression::Call { name, .. } if name == "visited"));
    }

    #[test]
    fn parse_count_call() {
        let e = parse_expression("count(\"forest\")").unwrap();
        assert!(matches!(e, Expression::Call { name, .. } if name == "count"));
    }
}
