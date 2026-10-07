use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) struct ParsedExpr {
    pub program: crate::parser::ParsedProgram,
    pub id: ExprId,
}

pub(super) struct ParsedStmt {
    pub program: crate::parser::ParsedProgram,
    pub id: StmtId,
}

pub(super) fn parse_expr(src: &str) -> ParsedExpr {
    let stream = Lexer::new(&format!("<?php {src};")).lex_token_stream();
    let program = Parser::from_token_stream(stream).parse_program().unwrap();
    let id = match &program.arenas.stmt(program.statements()[0]).unwrap().kind {
        StmtKind::Expression(id) => *id,
        other => panic!("expected an Expression statement, got {other:?}"),
    };
    ParsedExpr { program, id }
}

pub(super) fn parse_stmt(src: &str) -> ParsedStmt {
    let stream = Lexer::new(&format!("<?php {src}")).lex_token_stream();
    let program = Parser::from_token_stream(stream).parse_program().unwrap();
    ParsedStmt {
        id: program.statements()[0],
        program,
    }
}
