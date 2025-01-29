use chumsky::{prelude::*, select, Parser};
use logos::Logos;
use scan_core::Pmtl;

#[derive(Logos, Debug, PartialEq, Eq, Hash, Clone)]
#[logos(skip r"[ \t\n]+")]
#[logos(error = String)]
pub enum Token {
    #[token("P")]
    #[token("once")]
    Once,

    #[token("H")]
    #[token("historically")]
    Historically,

    #[token("S")]
    #[token("since")]
    Since,

    #[token("&&")]
    #[token("and")]
    And,

    #[token("||")]
    #[token("or")]
    Or,

    #[token("!")]
    #[token("not")]
    Not,

    #[token("->")]
    #[token("implies")]
    Implies,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token("[")]
    BracketOpen,

    #[token("]")]
    BracketClose,

    // #[token(",")]
    // Comma,
    #[token(":")]
    Colon,

    #[regex("[0-9]+", |lex| lex.slice().parse::<usize>().unwrap())]
    Integer(usize),

    #[regex(r#"\{[^{}]*\}"#, |lex| lex.slice().strip_prefix("{").unwrap().strip_suffix("}").unwrap().trim().to_owned())]
    Predicate(String),
}

fn parser() -> impl Parser<Token, Pmtl<String>, Error = Simple<Token>> {
    let integer = select! {
        Token::Integer(n) => n as u32,
    };

    recursive(|p| {
        let atom = {
            let parenthesized = p
                .clone()
                .delimited_by(just(Token::LParen), just(Token::RParen));

            let predicate = select! {
                Token::Predicate(pred) => Pmtl::Atom(pred),
            };

            parenthesized.or(predicate)
        };

        let unary = just(Token::Not)
            .or(just(Token::Once))
            .or(just(Token::Historically))
            .repeated()
            .then(atom)
            .foldr(|op, rhs| match op {
                Token::Not => Pmtl::Not(Box::new(rhs)),
                Token::Once => Pmtl::Once(Box::new(rhs), 0, u32::MAX),
                Token::Historically => Pmtl::Historically(Box::new(rhs), 0, u32::MAX),
                _ => unreachable!(),
            });

        let temp_unary = just(Token::Once)
            .or(just(Token::Historically))
            .then(
                just(Token::BracketOpen)
                    .ignore_then(integer)
                    .then_ignore(just(Token::Colon))
                    .then(integer)
                    .then_ignore(just(Token::BracketClose)),
            )
            .repeated()
            .then(unary)
            .foldr(|(op, (l, u)), rhs| match op {
                Token::Once => Pmtl::Once(Box::new(rhs), l, u),
                Token::Historically => Pmtl::Historically(Box::new(rhs), l, u),
                _ => unreachable!(),
            });

        let binary = temp_unary
            .clone()
            .then(
                just(Token::And)
                    .or(just(Token::Or))
                    .or(just(Token::Implies))
                    .or(just(Token::Since))
                    .then(temp_unary)
                    .repeated(),
            )
            .foldl(|lhs, (op, rhs)| match op {
                Token::And => Pmtl::And(vec![lhs, rhs]),
                Token::Or => Pmtl::Or(vec![lhs, rhs]),
                Token::Implies => Pmtl::Implies(Box::new((lhs, rhs))),
                Token::Since => Pmtl::Since(Box::new((lhs, rhs)), 0, u32::MAX),
                _ => unreachable!(),
            });

        binary
            .clone()
            .then(
                just(Token::Since)
                    .then_ignore(just(Token::BracketOpen))
                    .then(integer)
                    .then_ignore(just(Token::Colon))
                    .then(integer)
                    .then_ignore(just(Token::BracketClose))
                    .then(binary)
                    .repeated(),
            )
            .foldl(|lhs, (((_op, l), u), rhs)| Pmtl::Since(Box::new((lhs, rhs)), l, u))
    })
    .then_ignore(end())
}

pub fn parse_rye(input: &str) -> Result<Pmtl<String>, Vec<chumsky::error::Simple<Token>>> {
    //creates a lexer instance from the input
    let lexer = Token::lexer(input);

    //splits the input into tokens, using the lexer
    let mut tokens = vec![];
    for (token, span) in lexer.spanned() {
        match token {
            Ok(token) => tokens.push(token),
            Err(e) => {
                panic!("lexer error at {:?}: {}", span, e);
                // return;
            }
        }
    }

    //parses the tokens to construct an AST
    parser().parse(tokens)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn not() {
        let not = parse_rye("not { var > 10 }").expect("parsed formula");
        assert!(matches!(not, Pmtl::Not(_)));
        if let Pmtl::Not(atom) = not {
            assert!(matches!(*atom, Pmtl::Atom(_)));
            if let Pmtl::Atom(pred) = *atom {
                assert_eq!(pred, "var > 10".to_string());
            } else {
                unreachable!();
            }
        } else {
            unreachable!();
        }
    }

    #[test]
    fn bounded_once() {
        let once = parse_rye("P[0:1] { var > 10 }").expect("parsed formula");
        assert!(matches!(once, Pmtl::Once(_, 0, 1)));
        if let Pmtl::Once(atom, 0, 1) = once {
            assert!(matches!(*atom, Pmtl::Atom(_)));
            if let Pmtl::Atom(pred) = *atom {
                assert_eq!(pred, "var > 10".to_string());
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn unbounded_once() {
        let once = parse_rye("P { var > 10 }").expect("parsed formula");
        assert!(matches!(once, Pmtl::Once(_, 0, u32::MAX)));
        if let Pmtl::Once(atom, 0, u32::MAX) = once {
            assert!(matches!(*atom, Pmtl::Atom(_)));
            if let Pmtl::Atom(pred) = *atom {
                assert_eq!(pred, "var > 10".to_string());
            } else {
                unreachable!();
            }
        }
    }

    #[test]
    fn historically_once() {
        let historically = parse_rye("H[0:1] P[2:3] { var > 10 }").expect("parsed formula");
        assert!(matches!(historically, Pmtl::Historically(_, 0, 1)));
        if let Pmtl::Historically(once, 0, 1) = historically {
            assert!(matches!(*once, Pmtl::Once(_, 2, 3)));
        } else {
            unreachable!();
        }
    }

    #[test]
    fn since() {
        let since =
            parse_rye("not { var > 10 } since[2:10] { other_var == 1 }").expect("parsed formula");
        assert!(matches!(since, Pmtl::Since(_, 2, 10)));
        if let Pmtl::Since(args, _, _) = since {
            let (lhs, rhs) = *args;
            assert!(matches!(lhs, Pmtl::Not(_)));
            assert!(matches!(rhs, Pmtl::Atom(_)));
        }
    }
}
