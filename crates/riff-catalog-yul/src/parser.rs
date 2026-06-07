//! Recursive-descent Yul parser over the shared AST. Accepts solc-emitted
//! `ir`/`irOptimized` text, hand-written `.yul`, and fe-emitted Yul.

use crate::ast::*;
use crate::error::YulParseError;
use crate::lexer::{Pos, Token, lex};

pub fn parse_object(src: &str) -> Result<Object, YulParseError> {
    let tokens = lex(src)?;
    let mut parser = Parser { tokens, at: 0 };
    let object = if parser.peek_keyword("object") {
        parser.object()?
    } else {
        // bare top-level block, wrapped (handy for snippets/tests)
        Object {
            name: String::new(),
            code: Code {
                block: parser.block()?,
            },
            sub_objects: Vec::new(),
        }
    };
    if parser.at < parser.tokens.len() {
        return Err(parser.error_here("trailing tokens after object"));
    }
    Ok(object)
}

struct Parser {
    tokens: Vec<(Token, Pos)>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at).map(|(token, _)| token)
    }

    fn pos(&self) -> Pos {
        self.tokens
            .get(self.at)
            .or_else(|| self.tokens.last())
            .map(|(_, pos)| *pos)
            .unwrap_or(Pos { line: 1, col: 1 })
    }

    fn error_here(&self, message: impl Into<String>) -> YulParseError {
        let pos = self.pos();
        YulParseError::new(pos.line, pos.col, message)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).map(|(token, _)| token.clone());
        if token.is_some() {
            self.at += 1;
        }
        token
    }

    fn expect(&mut self, expected: &Token, what: &str) -> Result<(), YulParseError> {
        match self.peek() {
            Some(token) if token == expected => {
                self.at += 1;
                Ok(())
            }
            other => Err(self.error_here(format!("expected {what}, found {other:?}"))),
        }
    }

    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(self.peek(), Some(Token::Ident(ident)) if ident == keyword)
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        if self.peek_keyword(keyword) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), YulParseError> {
        if self.eat_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error_here(format!("expected `{keyword}`")))
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, YulParseError> {
        match self.peek() {
            Some(Token::Ident(ident)) => {
                let ident = ident.clone();
                self.at += 1;
                Ok(ident)
            }
            other => Err(self.error_here(format!("expected {what}, found {other:?}"))),
        }
    }

    fn expect_string(&mut self, what: &str) -> Result<Vec<u8>, YulParseError> {
        match self.peek() {
            Some(Token::Str(bytes)) => {
                let bytes = bytes.clone();
                self.at += 1;
                Ok(bytes)
            }
            other => Err(self.error_here(format!("expected {what}, found {other:?}"))),
        }
    }

    fn object(&mut self) -> Result<Object, YulParseError> {
        self.expect_keyword("object")?;
        let name_bytes = self.expect_string("object name string")?;
        let name = String::from_utf8(name_bytes)
            .map_err(|_| self.error_here("object name is not valid UTF-8"))?;
        self.expect(&Token::LBrace, "`{`")?;
        self.expect_keyword("code")?;
        let code = Code {
            block: self.block()?,
        };
        let mut sub_objects = Vec::new();
        loop {
            if self.peek_keyword("object") {
                sub_objects.push(ObjectChild::Object(self.object()?));
            } else if self.eat_keyword("data") {
                // Data NAME is discarded: solc's irAst JSON has no name key
                // for YulData, and AST equality across both front doors is
                // the conformance contract (see ast.rs module docs).
                let _name = self.expect_string("data name string")?;
                let bytes = match self.bump() {
                    Some(Token::Str(bytes)) => bytes,
                    Some(Token::HexStr(bytes)) => bytes,
                    other => {
                        return Err(
                            self.error_here(format!("expected data payload, found {other:?}"))
                        );
                    }
                };
                sub_objects.push(ObjectChild::Data(Data::from_bytes(&bytes)));
            } else {
                break;
            }
        }
        self.expect(&Token::RBrace, "`}`")?;
        Ok(Object {
            name,
            code,
            sub_objects,
        })
    }

    fn block(&mut self) -> Result<Block, YulParseError> {
        self.expect(&Token::LBrace, "`{`")?;
        let mut statements = Vec::new();
        while self.peek() != Some(&Token::RBrace) {
            if self.peek().is_none() {
                return Err(self.error_here("unterminated block"));
            }
            statements.push(self.statement()?);
        }
        self.expect(&Token::RBrace, "`}`")?;
        Ok(Block { statements })
    }

    fn statement(&mut self) -> Result<Statement, YulParseError> {
        match self.peek() {
            Some(Token::LBrace) => Ok(Statement::Block(self.block()?)),
            Some(Token::Ident(ident)) => match ident.as_str() {
                "function" => self.function_definition(),
                "let" => self.variable_declaration(),
                "if" => {
                    self.at += 1;
                    Ok(Statement::If(If {
                        condition: self.expression()?,
                        body: self.block()?,
                    }))
                }
                "switch" => self.switch(),
                "for" => {
                    self.at += 1;
                    Ok(Statement::ForLoop(ForLoop {
                        pre: self.block()?,
                        condition: self.expression()?,
                        post: self.block()?,
                        body: self.block()?,
                    }))
                }
                "break" => {
                    self.at += 1;
                    Ok(Statement::Break)
                }
                "continue" => {
                    self.at += 1;
                    Ok(Statement::Continue)
                }
                "leave" => {
                    self.at += 1;
                    Ok(Statement::Leave)
                }
                _ => self.expression_or_assignment(),
            },
            other => Err(self.error_here(format!("expected statement, found {other:?}"))),
        }
    }

    fn function_definition(&mut self) -> Result<Statement, YulParseError> {
        self.expect_keyword("function")?;
        let name = self.expect_ident("function name")?;
        self.expect(&Token::LParen, "`(`")?;
        let mut parameters = Vec::new();
        if self.peek() != Some(&Token::RParen) {
            loop {
                parameters.push(self.typed_name()?);
                if self.peek() == Some(&Token::Comma) {
                    self.at += 1;
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen, "`)`")?;
        let mut return_variables = Vec::new();
        if self.peek() == Some(&Token::Arrow) {
            self.at += 1;
            loop {
                return_variables.push(self.typed_name()?);
                if self.peek() == Some(&Token::Comma) {
                    self.at += 1;
                } else {
                    break;
                }
            }
        }
        Ok(Statement::FunctionDefinition(FunctionDefinition {
            name,
            parameters,
            return_variables,
            body: self.block()?,
        }))
    }

    fn typed_name(&mut self) -> Result<TypedName, YulParseError> {
        let name = self.expect_ident("name")?;
        let ty = if self.peek() == Some(&Token::Colon) {
            self.at += 1;
            Some(self.expect_ident("type name")?)
        } else {
            None
        };
        Ok(TypedName { name, ty })
    }

    fn variable_declaration(&mut self) -> Result<Statement, YulParseError> {
        self.expect_keyword("let")?;
        let mut variables = vec![self.typed_name()?];
        while self.peek() == Some(&Token::Comma) {
            self.at += 1;
            variables.push(self.typed_name()?);
        }
        let value = if self.peek() == Some(&Token::Assign) {
            self.at += 1;
            Some(self.expression()?)
        } else {
            None
        };
        Ok(Statement::VariableDeclaration(VariableDeclaration {
            variables,
            value,
        }))
    }

    fn switch(&mut self) -> Result<Statement, YulParseError> {
        self.expect_keyword("switch")?;
        let expression = self.expression()?;
        let mut cases = Vec::new();
        loop {
            if self.eat_keyword("case") {
                let value = self.literal()?;
                cases.push(Case {
                    value: Some(value),
                    body: self.block()?,
                });
            } else if self.eat_keyword("default") {
                cases.push(Case {
                    value: None,
                    body: self.block()?,
                });
                break;
            } else {
                break;
            }
        }
        if cases.is_empty() {
            return Err(self.error_here("switch needs at least one case"));
        }
        Ok(Statement::Switch(Switch { expression, cases }))
    }

    fn expression_or_assignment(&mut self) -> Result<Statement, YulParseError> {
        let first = self.expect_ident("identifier")?;
        match self.peek() {
            Some(Token::LParen) => {
                let call = self.call_with_name(first)?;
                Ok(Statement::Expression(ExpressionStatement {
                    expression: Expression::FunctionCall(call),
                }))
            }
            Some(Token::Comma) | Some(Token::Assign) => {
                let mut variable_names = vec![Identifier { name: first }];
                while self.peek() == Some(&Token::Comma) {
                    self.at += 1;
                    variable_names.push(Identifier {
                        name: self.expect_ident("assignment target")?,
                    });
                }
                self.expect(&Token::Assign, "`:=`")?;
                Ok(Statement::Assignment(Assignment {
                    variable_names,
                    value: self.expression()?,
                }))
            }
            other => Err(self.error_here(format!(
                "expected `(`, `,` or `:=` after identifier, found {other:?}"
            ))),
        }
    }

    fn call_with_name(&mut self, name: String) -> Result<FunctionCall, YulParseError> {
        self.expect(&Token::LParen, "`(`")?;
        let mut arguments = Vec::new();
        if self.peek() != Some(&Token::RParen) {
            loop {
                arguments.push(self.expression()?);
                if self.peek() == Some(&Token::Comma) {
                    self.at += 1;
                } else {
                    break;
                }
            }
        }
        self.expect(&Token::RParen, "`)`")?;
        Ok(FunctionCall {
            function_name: Identifier { name },
            arguments,
        })
    }

    fn expression(&mut self) -> Result<Expression, YulParseError> {
        match self.peek() {
            Some(Token::Number(_)) | Some(Token::Str(_)) | Some(Token::HexStr(_)) => {
                Ok(Expression::Literal(self.literal()?))
            }
            Some(Token::Ident(ident)) if ident == "true" || ident == "false" => {
                Ok(Expression::Literal(self.literal()?))
            }
            Some(Token::Ident(_)) => {
                let name = self.expect_ident("identifier")?;
                if self.peek() == Some(&Token::LParen) {
                    Ok(Expression::FunctionCall(self.call_with_name(name)?))
                } else {
                    Ok(Expression::Identifier(Identifier { name }))
                }
            }
            other => Err(self.error_here(format!("expected expression, found {other:?}"))),
        }
    }

    fn literal(&mut self) -> Result<Literal, YulParseError> {
        let mut literal = match self.bump() {
            Some(Token::Number(spelling)) => Literal::number(spelling),
            Some(Token::Str(bytes)) => Literal::string_from_bytes(&bytes),
            Some(Token::HexStr(bytes)) => Literal::string_from_bytes(&bytes),
            Some(Token::Ident(ident)) if ident == "true" => Literal::bool(true),
            Some(Token::Ident(ident)) if ident == "false" => Literal::bool(false),
            other => return Err(self.error_here(format!("expected literal, found {other:?}"))),
        };
        if self.peek() == Some(&Token::Colon) {
            self.at += 1;
            literal.ty = Some(self.expect_ident("literal type")?);
        }
        Ok(literal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_object() {
        let object =
            parse_object(r#"object "Demo" { code { let x := add(1, 0x02) sstore(0, x) } }"#)
                .unwrap();
        assert_eq!(object.name, "Demo");
        assert_eq!(object.code.block.statements.len(), 2);
    }

    #[test]
    fn parses_control_flow() {
        let src = r#"
        object "C" { code {
            function f(a, b) -> r {
                for { let i := 0 } lt(i, a) { i := add(i, 1) } {
                    if eq(i, b) { break }
                    switch i
                    case 0 { r := 1 }
                    case 0x10 { continue }
                    default { leave }
                }
            }
            let z := f(3, true)
        } }"#;
        let object = parse_object(src).unwrap();
        assert_eq!(object.code.block.statements.len(), 2);
    }

    #[test]
    fn parses_nested_objects_and_data() {
        let src = r#"
        object "A" {
            code { let x := datasize("B") }
            object "B" { code {} }
            data ".metadata" hex"a26469"
            data "text" "hi\n"
        }"#;
        let object = parse_object(src).unwrap();
        assert_eq!(object.sub_objects.len(), 3);
        match &object.sub_objects[1] {
            ObjectChild::Data(data) => assert_eq!(data.value, "0xa26469"),
            other => panic!("expected data, got {other:?}"),
        }
        match &object.sub_objects[2] {
            ObjectChild::Data(data) => assert_eq!(data.value, "0x68690a"),
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn parses_multi_assignment_and_typed_literals() {
        let src = r#"{ let a, b := f() a, b := g(a) let c := 1:u256 }"#;
        let object = parse_object(src).unwrap();
        assert_eq!(object.name, "");
        assert_eq!(object.code.block.statements.len(), 3);
        match &object.code.block.statements[2] {
            Statement::VariableDeclaration(decl) => match decl.value.as_ref().unwrap() {
                Expression::Literal(literal) => {
                    assert_eq!(literal.ty.as_deref(), Some("u256"));
                }
                other => panic!("expected literal, got {other:?}"),
            },
            other => panic!("expected let, got {other:?}"),
        }
    }

    #[test]
    fn statement_position_rejects_bare_identifier() {
        assert!(parse_object("{ x }").is_err());
    }
}
