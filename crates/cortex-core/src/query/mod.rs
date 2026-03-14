//! Lightweight query DSL -- a string-based filter expression that compiles to `NodeFilter`.
//!
//! ```text
//! kind:decision AND importance>0.7
//! tags:backend,rust AND agent:kai
//! created_after:7d AND kind:fact
//! importance>=0.5 AND NOT kind:event
//! (kind:decision OR kind:pattern) AND tags:architecture
//! ```

use crate::storage::NodeFilter;
use crate::types::NodeKind;
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Comparison operator for numeric fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CmpOp {
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
}

/// A single field filter.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldFilter {
    Kind(Vec<String>),
    Tags(Vec<String>),
    Agent(String),
    Importance { op: CmpOp, value: f32 },
    CreatedAfter(DateTime<Utc>),
    CreatedBefore(DateTime<Utc>),
    Deleted(bool),
    Limit(usize),
}

/// AST for filter expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterExpr {
    Field(FieldFilter),
    And(Box<FilterExpr>, Box<FilterExpr>),
    Or(Box<FilterExpr>, Box<FilterExpr>),
    Not(Box<FilterExpr>),
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at position {}: {}",
            self.position, self.message
        )
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Compile error: {}", self.message)
    }
}

impl std::error::Error for CompileError {}

#[derive(Debug)]
pub enum QueryError {
    Parse(ParseError),
    Compile(CompileError),
}

impl std::fmt::Display for QueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryError::Parse(e) => write!(f, "{}", e),
            QueryError::Compile(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for QueryError {}

impl From<ParseError> for QueryError {
    fn from(e: ParseError) -> Self {
        QueryError::Parse(e)
    }
}

impl From<CompileError> for QueryError {
    fn from(e: CompileError) -> Self {
        QueryError::Compile(e)
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Colon,
    Comma,
    LParen,
    RParen,
    Op(CmpOp),
    Number(f64),
    And,
    Or,
    Not,
}

/// Position-tagged token used for error reporting.
#[derive(Debug, Clone)]
struct PosToken {
    token: Token,
    pos: usize,
}

fn tokenize(input: &str) -> Result<Vec<PosToken>, ParseError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        if chars[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let start = i;

        match chars[i] {
            ':' => {
                tokens.push(PosToken {
                    token: Token::Colon,
                    pos: start,
                });
                i += 1;
            }
            ',' => {
                tokens.push(PosToken {
                    token: Token::Comma,
                    pos: start,
                });
                i += 1;
            }
            '(' => {
                tokens.push(PosToken {
                    token: Token::LParen,
                    pos: start,
                });
                i += 1;
            }
            ')' => {
                tokens.push(PosToken {
                    token: Token::RParen,
                    pos: start,
                });
                i += 1;
            }
            '>' => {
                if i + 1 < len && chars[i + 1] == '=' {
                    tokens.push(PosToken {
                        token: Token::Op(CmpOp::Gte),
                        pos: start,
                    });
                    i += 2;
                } else {
                    tokens.push(PosToken {
                        token: Token::Op(CmpOp::Gt),
                        pos: start,
                    });
                    i += 1;
                }
            }
            '<' => {
                if i + 1 < len && chars[i + 1] == '=' {
                    tokens.push(PosToken {
                        token: Token::Op(CmpOp::Lte),
                        pos: start,
                    });
                    i += 2;
                } else {
                    tokens.push(PosToken {
                        token: Token::Op(CmpOp::Lt),
                        pos: start,
                    });
                    i += 1;
                }
            }
            '=' => {
                tokens.push(PosToken {
                    token: Token::Op(CmpOp::Eq),
                    pos: start,
                });
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                // Attempt to parse a number. If followed by alpha (like "7d"), we
                // treat the whole thing as a word instead.
                let num_start = i;
                while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                // Check if this is a duration/date suffix (letter follows)
                if i < len && chars[i].is_ascii_alphabetic() {
                    // It is something like 7d or 24h or an ISO date fragment --
                    // consume the rest as a word.
                    while i < len
                        && !chars[i].is_ascii_whitespace()
                        && chars[i] != ','
                        && chars[i] != ')'
                        && chars[i] != '('
                    {
                        i += 1;
                    }
                    let word: String = chars[num_start..i].iter().collect();
                    tokens.push(PosToken {
                        token: Token::Word(word),
                        pos: start,
                    });
                } else {
                    let num_str: String = chars[num_start..i].iter().collect();
                    let value: f64 = num_str.parse().map_err(|_| ParseError {
                        message: format!("Invalid number: {}", num_str),
                        position: start,
                    })?;
                    tokens.push(PosToken {
                        token: Token::Number(value),
                        pos: start,
                    });
                }
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '-' => {
                let word_start = i;
                while i < len
                    && (chars[i].is_ascii_alphanumeric()
                        || chars[i] == '_'
                        || chars[i] == '-'
                        || chars[i] == '.')
                {
                    i += 1;
                }
                let word: String = chars[word_start..i].iter().collect();
                match word.as_str() {
                    "AND" => tokens.push(PosToken {
                        token: Token::And,
                        pos: start,
                    }),
                    "OR" => tokens.push(PosToken {
                        token: Token::Or,
                        pos: start,
                    }),
                    "NOT" => tokens.push(PosToken {
                        token: Token::Not,
                        pos: start,
                    }),
                    _ => tokens.push(PosToken {
                        token: Token::Word(word),
                        pos: start,
                    }),
                }
            }
            other => {
                return Err(ParseError {
                    message: format!("Unexpected character: '{}'", other),
                    position: start,
                });
            }
        }
    }

    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<PosToken>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<PosToken>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn current_pos(&self) -> usize {
        if self.pos < self.tokens.len() {
            self.tokens[self.pos].pos
        } else if let Some(last) = self.tokens.last() {
            last.pos + 1
        } else {
            0
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    fn advance(&mut self) -> Option<&PosToken> {
        if self.pos < self.tokens.len() {
            let t = &self.tokens[self.pos];
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<(), ParseError> {
        match self.peek() {
            Some(t) if t == expected => {
                self.advance();
                Ok(())
            }
            Some(t) => Err(ParseError {
                message: format!("Expected {:?}, found {:?}", expected, t),
                position: self.current_pos(),
            }),
            None => Err(ParseError {
                message: format!("Expected {:?}, found end of input", expected),
                position: self.current_pos(),
            }),
        }
    }

    /// Top-level entry: `expr = or_expr`
    fn parse_expr(&mut self) -> Result<FilterExpr, ParseError> {
        self.parse_or()
    }

    /// `or_expr = and_expr ("OR" and_expr)*`
    fn parse_or(&mut self) -> Result<FilterExpr, ParseError> {
        let mut left = self.parse_and()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = FilterExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `and_expr = unary ("AND" unary)*`
    fn parse_and(&mut self) -> Result<FilterExpr, ParseError> {
        let mut left = self.parse_unary()?;
        while self.peek() == Some(&Token::And) {
            self.advance();
            let right = self.parse_unary()?;
            left = FilterExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `unary = "NOT" atom | atom`
    fn parse_unary(&mut self) -> Result<FilterExpr, ParseError> {
        if self.peek() == Some(&Token::Not) {
            self.advance();
            let inner = self.parse_atom()?;
            Ok(FilterExpr::Not(Box::new(inner)))
        } else {
            self.parse_atom()
        }
    }

    /// `atom = "(" expr ")" | field_expr`
    fn parse_atom(&mut self) -> Result<FilterExpr, ParseError> {
        if self.peek() == Some(&Token::LParen) {
            self.advance();
            let expr = self.parse_expr()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            self.parse_field_expr()
        }
    }

    /// Parse a field expression (e.g. `kind:decision`, `importance>0.7`, `tags:a,b`)
    fn parse_field_expr(&mut self) -> Result<FilterExpr, ParseError> {
        let pos = self.current_pos();
        let field_name = match self.advance() {
            Some(PosToken {
                token: Token::Word(w),
                ..
            }) => w.clone(),
            Some(pt) => {
                return Err(ParseError {
                    message: format!("Expected field name, found {:?}", pt.token),
                    position: pt.pos,
                });
            }
            None => {
                return Err(ParseError {
                    message: "Expected field name, found end of input".to_string(),
                    position: pos,
                });
            }
        };

        match field_name.as_str() {
            "kind" => {
                self.expect(&Token::Colon)?;
                let values = self.parse_comma_values()?;
                Ok(FilterExpr::Field(FieldFilter::Kind(values)))
            }
            "tags" => {
                self.expect(&Token::Colon)?;
                let values = self.parse_comma_values()?;
                Ok(FilterExpr::Field(FieldFilter::Tags(values)))
            }
            "agent" => {
                self.expect(&Token::Colon)?;
                let value = self.parse_value()?;
                Ok(FilterExpr::Field(FieldFilter::Agent(value)))
            }
            "importance" => {
                let op = self.parse_cmp_op()?;
                let num = self.parse_number()?;
                Ok(FilterExpr::Field(FieldFilter::Importance {
                    op,
                    value: num as f32,
                }))
            }
            "created_after" => {
                self.expect(&Token::Colon)?;
                let value = self.parse_value()?;
                let dt = parse_duration_or_date(&value, pos)?;
                Ok(FilterExpr::Field(FieldFilter::CreatedAfter(dt)))
            }
            "created_before" => {
                self.expect(&Token::Colon)?;
                let value = self.parse_value()?;
                let dt = parse_duration_or_date(&value, pos)?;
                Ok(FilterExpr::Field(FieldFilter::CreatedBefore(dt)))
            }
            "deleted" => {
                self.expect(&Token::Colon)?;
                let value = self.parse_value()?;
                let b = match value.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(ParseError {
                            message: format!(
                                "Expected 'true' or 'false' for deleted, found '{}'",
                                value
                            ),
                            position: pos,
                        });
                    }
                };
                Ok(FilterExpr::Field(FieldFilter::Deleted(b)))
            }
            "limit" => {
                self.expect(&Token::Colon)?;
                let num = self.parse_number()?;
                Ok(FilterExpr::Field(FieldFilter::Limit(num as usize)))
            }
            other => Err(ParseError {
                message: format!("Unknown field: '{}'", other),
                position: pos,
            }),
        }
    }

    /// Parse a comparison operator token.
    fn parse_cmp_op(&mut self) -> Result<CmpOp, ParseError> {
        let pos = self.current_pos();
        match self.advance() {
            Some(PosToken {
                token: Token::Op(op),
                ..
            }) => Ok(*op),
            Some(pt) => Err(ParseError {
                message: format!("Expected comparison operator, found {:?}", pt.token),
                position: pt.pos,
            }),
            None => Err(ParseError {
                message: "Expected comparison operator, found end of input".to_string(),
                position: pos,
            }),
        }
    }

    /// Parse a numeric value from either a `Number` token or a `Word` token containing digits.
    fn parse_number(&mut self) -> Result<f64, ParseError> {
        let pos = self.current_pos();
        match self.advance() {
            Some(PosToken {
                token: Token::Number(n),
                ..
            }) => Ok(*n),
            Some(PosToken {
                token: Token::Word(w),
                ..
            }) => w.parse::<f64>().map_err(|_| ParseError {
                message: format!("Expected number, found '{}'", w),
                position: pos,
            }),
            Some(pt) => Err(ParseError {
                message: format!("Expected number, found {:?}", pt.token),
                position: pt.pos,
            }),
            None => Err(ParseError {
                message: "Expected number, found end of input".to_string(),
                position: pos,
            }),
        }
    }

    /// Parse a single word value.
    fn parse_value(&mut self) -> Result<String, ParseError> {
        let pos = self.current_pos();
        match self.advance() {
            Some(PosToken {
                token: Token::Word(w),
                ..
            }) => Ok(w.clone()),
            Some(PosToken {
                token: Token::Number(n),
                ..
            }) => {
                // Allow numbers as values (e.g. limit:10)
                if *n == (*n as u64) as f64 {
                    Ok(format!("{}", *n as u64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Some(pt) => Err(ParseError {
                message: format!("Expected value, found {:?}", pt.token),
                position: pt.pos,
            }),
            None => Err(ParseError {
                message: "Expected value, found end of input".to_string(),
                position: pos,
            }),
        }
    }

    /// Parse comma-separated values: `value ("," value)*`
    fn parse_comma_values(&mut self) -> Result<Vec<String>, ParseError> {
        let mut values = vec![self.parse_value()?];
        while self.peek() == Some(&Token::Comma) {
            self.advance();
            values.push(self.parse_value()?);
        }
        Ok(values)
    }
}

// ---------------------------------------------------------------------------
// Duration / date parsing
// ---------------------------------------------------------------------------

/// Parse a relative duration (`7d`, `24h`, `30m`) or an ISO-8601 date string.
/// Durations are computed as `Utc::now() - duration`.
fn parse_duration_or_date(value: &str, pos: usize) -> Result<DateTime<Utc>, ParseError> {
    let len = value.len();
    if len < 2 {
        // Try ISO-8601 parse
        return value.parse::<DateTime<Utc>>().map_err(|_| ParseError {
            message: format!(
                "Invalid duration or date: '{}'. Use 7d, 24h, 30m, or ISO-8601.",
                value
            ),
            position: pos,
        });
    }

    let suffix = &value[len - 1..];
    let num_part = &value[..len - 1];

    match suffix {
        "d" | "h" | "m" => {
            let n: i64 = num_part.parse().map_err(|_| ParseError {
                message: format!("Invalid number in duration: '{}'", num_part),
                position: pos,
            })?;
            let duration = match suffix {
                "d" => chrono::Duration::days(n),
                "h" => chrono::Duration::hours(n),
                "m" => chrono::Duration::minutes(n),
                _ => unreachable!(),
            };
            Ok(Utc::now() - duration)
        }
        _ => {
            // Try ISO-8601 parse
            value.parse::<DateTime<Utc>>().map_err(|_| ParseError {
                message: format!(
                    "Invalid duration or date: '{}'. Use 7d, 24h, 30m, or ISO-8601.",
                    value
                ),
                position: pos,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Public API: parse
// ---------------------------------------------------------------------------

/// Parse a filter expression string into an AST.
pub fn parse(input: &str) -> Result<FilterExpr, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError {
            message: "Empty filter expression".to_string(),
            position: 0,
        });
    }

    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(ParseError {
            message: "Empty filter expression".to_string(),
            position: 0,
        });
    }

    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;

    // Ensure all tokens were consumed
    if parser.pos < parser.tokens.len() {
        return Err(ParseError {
            message: format!(
                "Unexpected token: {:?}",
                parser.tokens[parser.pos].token
            ),
            position: parser.tokens[parser.pos].pos,
        });
    }

    Ok(expr)
}

// ---------------------------------------------------------------------------
// Compiler: AST -> NodeFilter
// ---------------------------------------------------------------------------

/// Compile a `FilterExpr` AST into a `NodeFilter`.
///
/// Supports:
/// - Simple field expressions
/// - AND-chains (merged into a single `NodeFilter`)
/// - OR at the field level within the same field type (e.g. `kind:a OR kind:b`)
/// - NOT for `Deleted` (flips the boolean)
///
/// Returns `CompileError` for unsupported OR/NOT combinations.
pub fn compile(expr: &FilterExpr) -> Result<NodeFilter, CompileError> {
    let mut filter = NodeFilter::default();
    collect_into(expr, &mut filter)?;
    Ok(filter)
}

/// Recursively collect field filters from the AST into a single `NodeFilter`.
fn collect_into(expr: &FilterExpr, filter: &mut NodeFilter) -> Result<(), CompileError> {
    match expr {
        FilterExpr::Field(field) => {
            apply_field(field, filter)?;
        }
        FilterExpr::And(left, right) => {
            collect_into(left, filter)?;
            collect_into(right, filter)?;
        }
        FilterExpr::Or(left, right) => {
            // OR is only supported when both sides are the same field type (Kind or Tags).
            match (left.as_ref(), right.as_ref()) {
                (FilterExpr::Field(FieldFilter::Kind(a)), FilterExpr::Field(FieldFilter::Kind(b))) => {
                    let mut merged = a.clone();
                    merged.extend(b.iter().cloned());
                    apply_field(&FieldFilter::Kind(merged), filter)?;
                }
                (FilterExpr::Field(FieldFilter::Tags(a)), FilterExpr::Field(FieldFilter::Tags(b))) => {
                    let mut merged = a.clone();
                    merged.extend(b.iter().cloned());
                    apply_field(&FieldFilter::Tags(merged), filter)?;
                }
                // Nested OR of same kind fields: (kind:a OR kind:b) OR kind:c
                (FilterExpr::Or(_, _), FilterExpr::Field(FieldFilter::Kind(_)))
                | (FilterExpr::Field(FieldFilter::Kind(_)), FilterExpr::Or(_, _))
                | (FilterExpr::Or(_, _), FilterExpr::Or(_, _)) => {
                    // Try to flatten: collect kinds from both sides
                    let mut kinds_left = Vec::new();
                    if try_collect_kinds(left, &mut kinds_left) {
                        let mut kinds_right = Vec::new();
                        if try_collect_kinds(right, &mut kinds_right) {
                            kinds_left.extend(kinds_right);
                            apply_field(&FieldFilter::Kind(kinds_left), filter)?;
                            return Ok(());
                        }
                    }
                    return Err(CompileError {
                        message: "OR is only supported between the same field type \
                                  (e.g. kind:a OR kind:b). Complex OR expressions \
                                  cannot be compiled to a single NodeFilter."
                            .to_string(),
                    });
                }
                _ => {
                    return Err(CompileError {
                        message: "OR is only supported between the same field type \
                                  (e.g. kind:a OR kind:b). Complex OR expressions \
                                  cannot be compiled to a single NodeFilter."
                            .to_string(),
                    });
                }
            }
        }
        FilterExpr::Not(inner) => match inner.as_ref() {
            FilterExpr::Field(FieldFilter::Deleted(b)) => {
                apply_field(&FieldFilter::Deleted(!b), filter)?;
            }
            _ => {
                return Err(CompileError {
                    message: "NOT is only supported for the 'deleted' field. \
                              Negation of other fields cannot be represented \
                              in a NodeFilter."
                        .to_string(),
                });
            }
        },
    }
    Ok(())
}

/// Try to recursively collect Kind values from an OR tree.
/// Returns true if the entire sub-tree is an OR chain of Kind fields.
fn try_collect_kinds(expr: &FilterExpr, out: &mut Vec<String>) -> bool {
    match expr {
        FilterExpr::Field(FieldFilter::Kind(values)) => {
            out.extend(values.iter().cloned());
            true
        }
        FilterExpr::Or(left, right) => {
            try_collect_kinds(left, out) && try_collect_kinds(right, out)
        }
        _ => false,
    }
}

/// Apply a single `FieldFilter` to a `NodeFilter`, merging with any existing values.
fn apply_field(field: &FieldFilter, filter: &mut NodeFilter) -> Result<(), CompileError> {
    match field {
        FieldFilter::Kind(values) => {
            let kinds: Result<Vec<NodeKind>, _> =
                values.iter().map(|v| NodeKind::new(v)).collect();
            let kinds = kinds.map_err(|e| CompileError {
                message: format!("Invalid node kind: {}", e),
            })?;
            match filter.kinds.as_mut() {
                Some(existing) => existing.extend(kinds),
                None => filter.kinds = Some(kinds),
            }
        }
        FieldFilter::Tags(values) => match filter.tags.as_mut() {
            Some(existing) => {
                for v in values {
                    existing.push(v.clone());
                }
            }
            None => filter.tags = Some(values.clone()),
        },
        FieldFilter::Agent(value) => {
            filter.source_agent = Some(value.clone());
        }
        FieldFilter::Importance { op, value } => match op {
            CmpOp::Gt | CmpOp::Gte => {
                filter.min_importance = Some(*value);
            }
            CmpOp::Eq => {
                // For equality, treat as min_importance (best approximation)
                filter.min_importance = Some(*value);
            }
            CmpOp::Lt | CmpOp::Lte => {
                // NodeFilter only has min_importance; LT/LTE cannot be represented directly.
                return Err(CompileError {
                    message: format!(
                        "importance{}{} cannot be compiled: NodeFilter only supports \
                         minimum importance (>, >=, =)",
                        match op {
                            CmpOp::Lt => "<",
                            CmpOp::Lte => "<=",
                            _ => unreachable!(),
                        },
                        value
                    ),
                });
            }
        },
        FieldFilter::CreatedAfter(dt) => {
            filter.created_after = Some(*dt);
        }
        FieldFilter::CreatedBefore(dt) => {
            filter.created_before = Some(*dt);
        }
        FieldFilter::Deleted(b) => {
            if *b {
                filter.deleted_only = true;
                filter.include_deleted = true;
            } else {
                filter.deleted_only = false;
                filter.include_deleted = false;
            }
        }
        FieldFilter::Limit(n) => {
            filter.limit = Some(*n);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Convenience function
// ---------------------------------------------------------------------------

/// Parse a filter expression string and compile it to a `NodeFilter` in one step.
pub fn parse_and_compile(input: &str) -> Result<NodeFilter, QueryError> {
    let expr = parse(input)?;
    let filter = compile(&expr)?;
    Ok(filter)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_kind() {
        let expr = parse("kind:decision").unwrap();
        assert_eq!(
            expr,
            FilterExpr::Field(FieldFilter::Kind(vec!["decision".to_string()]))
        );
    }

    #[test]
    fn test_parse_tags_comma_separated() {
        let expr = parse("tags:backend,rust").unwrap();
        assert_eq!(
            expr,
            FilterExpr::Field(FieldFilter::Tags(vec![
                "backend".to_string(),
                "rust".to_string()
            ]))
        );
    }

    #[test]
    fn test_parse_agent() {
        let expr = parse("agent:kai").unwrap();
        assert_eq!(
            expr,
            FilterExpr::Field(FieldFilter::Agent("kai".to_string()))
        );
    }

    #[test]
    fn test_parse_importance_gt() {
        let expr = parse("importance>0.7").unwrap();
        assert_eq!(
            expr,
            FilterExpr::Field(FieldFilter::Importance {
                op: CmpOp::Gt,
                value: 0.7
            })
        );
    }

    #[test]
    fn test_parse_importance_gte() {
        let expr = parse("importance>=0.5").unwrap();
        assert_eq!(
            expr,
            FilterExpr::Field(FieldFilter::Importance {
                op: CmpOp::Gte,
                value: 0.5
            })
        );
    }

    #[test]
    fn test_parse_and_expression() {
        let expr = parse("kind:decision AND importance>0.7").unwrap();
        match expr {
            FilterExpr::And(left, right) => {
                assert_eq!(
                    *left,
                    FilterExpr::Field(FieldFilter::Kind(vec!["decision".to_string()]))
                );
                assert_eq!(
                    *right,
                    FilterExpr::Field(FieldFilter::Importance {
                        op: CmpOp::Gt,
                        value: 0.7
                    })
                );
            }
            _ => panic!("Expected And expression"),
        }
    }

    #[test]
    fn test_parse_not_expression() {
        let expr = parse("NOT kind:event").unwrap();
        match expr {
            FilterExpr::Not(inner) => {
                assert_eq!(
                    *inner,
                    FilterExpr::Field(FieldFilter::Kind(vec!["event".to_string()]))
                );
            }
            _ => panic!("Expected Not expression"),
        }
    }

    #[test]
    fn test_parse_or_expression() {
        let expr = parse("kind:decision OR kind:pattern").unwrap();
        match expr {
            FilterExpr::Or(left, right) => {
                assert_eq!(
                    *left,
                    FilterExpr::Field(FieldFilter::Kind(vec!["decision".to_string()]))
                );
                assert_eq!(
                    *right,
                    FilterExpr::Field(FieldFilter::Kind(vec!["pattern".to_string()]))
                );
            }
            _ => panic!("Expected Or expression"),
        }
    }

    #[test]
    fn test_parse_parenthesized() {
        let expr = parse("(kind:decision OR kind:pattern) AND tags:architecture").unwrap();
        match expr {
            FilterExpr::And(left, right) => {
                assert!(matches!(*left, FilterExpr::Or(_, _)));
                assert_eq!(
                    *right,
                    FilterExpr::Field(FieldFilter::Tags(vec!["architecture".to_string()]))
                );
            }
            _ => panic!("Expected And expression"),
        }
    }

    #[test]
    fn test_parse_deleted() {
        let expr = parse("deleted:true").unwrap();
        assert_eq!(expr, FilterExpr::Field(FieldFilter::Deleted(true)));
    }

    #[test]
    fn test_parse_limit() {
        let expr = parse("limit:10").unwrap();
        assert_eq!(expr, FilterExpr::Field(FieldFilter::Limit(10)));
    }

    #[test]
    fn test_parse_duration_days() {
        let expr = parse("created_after:7d").unwrap();
        match expr {
            FilterExpr::Field(FieldFilter::CreatedAfter(dt)) => {
                let now = Utc::now();
                let diff = now - dt;
                // Should be approximately 7 days
                assert!(diff.num_days() >= 6 && diff.num_days() <= 8);
            }
            _ => panic!("Expected CreatedAfter"),
        }
    }

    #[test]
    fn test_parse_duration_hours() {
        let expr = parse("created_after:24h").unwrap();
        match expr {
            FilterExpr::Field(FieldFilter::CreatedAfter(dt)) => {
                let now = Utc::now();
                let diff = now - dt;
                assert!(diff.num_hours() >= 23 && diff.num_hours() <= 25);
            }
            _ => panic!("Expected CreatedAfter"),
        }
    }

    #[test]
    fn test_compile_simple_kind() {
        let filter = parse_and_compile("kind:decision").unwrap();
        assert_eq!(filter.kinds.unwrap().len(), 1);
    }

    #[test]
    fn test_compile_and_chain() {
        let filter = parse_and_compile("kind:decision AND agent:kai AND limit:5").unwrap();
        assert!(filter.kinds.is_some());
        assert_eq!(filter.source_agent, Some("kai".to_string()));
        assert_eq!(filter.limit, Some(5));
    }

    #[test]
    fn test_compile_importance() {
        let filter = parse_and_compile("importance>0.7").unwrap();
        // importance>0.7 maps to min_importance (only GT/GTE are meaningful for NodeFilter)
        assert!(filter.min_importance.is_some());
    }

    #[test]
    fn test_parse_error_invalid_field() {
        let result = parse("invalid_field:value");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_or_same_field_kind() {
        // OR of same field type should merge
        let filter = parse_and_compile("kind:decision OR kind:pattern").unwrap();
        let kinds = filter.kinds.unwrap();
        assert_eq!(kinds.len(), 2);
    }

    #[test]
    fn test_operator_precedence() {
        // AND binds tighter than OR
        let expr = parse("kind:decision OR kind:pattern AND tags:arch").unwrap();
        // Should parse as: kind:decision OR (kind:pattern AND tags:arch)
        match expr {
            FilterExpr::Or(_, right) => {
                assert!(matches!(*right, FilterExpr::And(_, _)));
            }
            _ => panic!("Expected OR at top level"),
        }
    }

    #[test]
    fn test_parse_empty_input() {
        let result = parse("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_whitespace_only() {
        let result = parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_deleted_true() {
        let filter = parse_and_compile("deleted:true").unwrap();
        assert!(filter.deleted_only);
        assert!(filter.include_deleted);
    }

    #[test]
    fn test_compile_deleted_false() {
        let filter = parse_and_compile("deleted:false").unwrap();
        assert!(!filter.deleted_only);
        assert!(!filter.include_deleted);
    }

    #[test]
    fn test_compile_not_deleted() {
        let filter = parse_and_compile("NOT deleted:true").unwrap();
        assert!(!filter.deleted_only);
        assert!(!filter.include_deleted);
    }

    #[test]
    fn test_compile_or_different_fields_fails() {
        let result = parse_and_compile("kind:decision OR agent:kai");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_not_kind_fails() {
        let result = parse_and_compile("NOT kind:decision");
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_importance_lt_fails() {
        let result = parse_and_compile("importance<0.5");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_duration_minutes() {
        let expr = parse("created_after:30m").unwrap();
        match expr {
            FilterExpr::Field(FieldFilter::CreatedAfter(dt)) => {
                let now = Utc::now();
                let diff = now - dt;
                assert!(diff.num_minutes() >= 29 && diff.num_minutes() <= 31);
            }
            _ => panic!("Expected CreatedAfter"),
        }
    }

    #[test]
    fn test_parse_created_before() {
        let expr = parse("created_before:7d").unwrap();
        match expr {
            FilterExpr::Field(FieldFilter::CreatedBefore(dt)) => {
                let now = Utc::now();
                let diff = now - dt;
                assert!(diff.num_days() >= 6 && diff.num_days() <= 8);
            }
            _ => panic!("Expected CreatedBefore"),
        }
    }

    #[test]
    fn test_complex_and_chain() {
        let filter =
            parse_and_compile("kind:fact AND tags:backend,rust AND agent:kai AND importance>=0.5")
                .unwrap();
        assert_eq!(filter.kinds.as_ref().unwrap().len(), 1);
        assert_eq!(filter.tags.as_ref().unwrap().len(), 2);
        assert_eq!(filter.source_agent, Some("kai".to_string()));
        assert_eq!(filter.min_importance, Some(0.5));
    }

    #[test]
    fn test_parenthesized_or_compiles() {
        // (kind:decision OR kind:pattern) AND tags:architecture
        let filter =
            parse_and_compile("(kind:decision OR kind:pattern) AND tags:architecture").unwrap();
        let kinds = filter.kinds.unwrap();
        assert_eq!(kinds.len(), 2);
        assert_eq!(
            filter.tags.unwrap(),
            vec!["architecture".to_string()]
        );
    }
}
