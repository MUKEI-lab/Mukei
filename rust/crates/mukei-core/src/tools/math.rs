//! Sandboxed math evaluator.

use std::collections::BTreeSet;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{MukeiError, Result};
use crate::tools::sentinel::{wrap_external_data, ExternalDataSource};
use crate::tools::Tool;

const MAX_EXPRESSION_BYTES: usize = 1024;
const TIMEOUT: Duration = Duration::from_secs(8);
const ALLOWED_IDENTIFIERS: &[&str] = &[
    "pi", "e", "abs", "sqrt", "cbrt", "exp", "ln", "log", "log10", "sin", "cos", "tan", "asin",
    "acos", "atan", "sinh", "cosh", "tanh", "floor", "ceil", "round", "signum", "min", "max",
];

#[derive(Default)]
pub struct MathTool;

#[derive(Debug, Clone, Deserialize)]
struct MathArgs {
    expression: String,
}

#[async_trait]
impl Tool for MathTool {
    fn name(&self) -> &'static str {
        "math_eval"
    }

    async fn run(&self, arguments: Value) -> Result<String> {
        let args: MathArgs = serde_json::from_value(arguments)
            .map_err(|error| MukeiError::ToolParseFailed(error.to_string()))?;
        if args.expression.len() > MAX_EXPRESSION_BYTES {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "expression",
                reason: format!("exceeds {MAX_EXPRESSION_BYTES} bytes"),
            });
        }
        let expression = args.expression.trim().to_string();
        validate_expression(&expression)?;

        let expression_for_eval = expression.clone();
        let mut join =
            crate::runtime::spawn_blocking_tool(move || evaluate_expression(&expression_for_eval));
        let result = match tokio::time::timeout(TIMEOUT, &mut join).await {
            Ok(Ok(inner)) => inner?,
            Ok(Err(error)) => return Err(MukeiError::BlockingJoinFailed(error.to_string())),
            Err(_) => {
                join.abort();
                tracing::warn!(timeout = ?TIMEOUT, "math_eval timeout — aborting JoinHandle");
                return Err(MukeiError::ToolTimeout(Some(TIMEOUT)));
            }
        };

        Ok(wrap_external_data(
            ExternalDataSource::Math,
            &format!("Expression: {expression}\nResult: {result}"),
        ))
    }
}

pub fn validate_expression(expression: &str) -> Result<()> {
    if expression.is_empty() {
        return Err(MukeiError::ToolArgumentInvalid {
            field: "expression",
            reason: "empty expression".to_string(),
        });
    }
    for character in expression.chars() {
        let valid = character.is_ascii_alphanumeric()
            || matches!(
                character,
                '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')' | '.' | ',' | ' ' | '\t' | '\n' | '_'
            );
        if !valid {
            return Err(MukeiError::SandboxViolation);
        }
    }

    for identifier in extract_identifiers(expression) {
        if !ALLOWED_IDENTIFIERS.contains(&identifier.as_str()) {
            return Err(MukeiError::ToolArgumentInvalid {
                field: "expression",
                reason: format!("identifier '{identifier}' is not whitelisted"),
            });
        }
    }
    Ok(())
}

fn extract_identifiers(expression: &str) -> BTreeSet<String> {
    let mut output = BTreeSet::new();
    let mut current = String::new();
    for character in expression.chars() {
        if character.is_ascii_alphabetic() || character == '_' {
            current.push(character);
        } else if !current.is_empty() {
            output.insert(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        output.insert(current);
    }
    output
}

fn evaluate_expression(expression: &str) -> Result<f64> {
    let mut parser = ArithmeticParser::new(expression);
    let value = parser.parse_expression()?;
    parser.skip_whitespace();
    if !parser.is_at_end() {
        return Err(math_parse_error("unexpected trailing input"));
    }
    Ok(value)
}

fn math_parse_error(reason: &'static str) -> MukeiError {
    MukeiError::ToolExecutionFailed(format!("invalid math expression: {reason}"))
}

struct ArithmeticParser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> ArithmeticParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    fn parse_expression(&mut self) -> Result<f64> {
        self.parse_add_sub()
    }

    fn parse_add_sub(&mut self) -> Result<f64> {
        let mut value = self.parse_mul_div()?;
        loop {
            self.skip_whitespace();
            if self.consume(b'+') {
                value += self.parse_mul_div()?;
            } else if self.consume(b'-') {
                value -= self.parse_mul_div()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_mul_div(&mut self) -> Result<f64> {
        let mut value = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            if self.consume(b'*') {
                value *= self.parse_unary()?;
            } else if self.consume(b'/') {
                value /= self.parse_unary()?;
            } else if self.consume(b'%') {
                value %= self.parse_unary()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<f64> {
        self.skip_whitespace();
        if self.consume(b'+') {
            self.parse_unary()
        } else if self.consume(b'-') {
            Ok(-self.parse_unary()?)
        } else {
            self.parse_power()
        }
    }

    fn parse_power(&mut self) -> Result<f64> {
        let base = self.parse_primary()?;
        self.skip_whitespace();
        if self.consume(b'^') {
            Ok(base.powf(self.parse_unary()?))
        } else {
            Ok(base)
        }
    }

    fn parse_primary(&mut self) -> Result<f64> {
        self.skip_whitespace();
        if self.consume(b'(') {
            let value = self.parse_expression()?;
            self.skip_whitespace();
            if !self.consume(b')') {
                return Err(math_parse_error("missing closing parenthesis"));
            }
            return Ok(value);
        }

        let start = self.position;
        while let Some(byte) = self.peek() {
            if byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E') {
                self.position += 1;
            } else if matches!(byte, b'+' | b'-')
                && self.position > start
                && matches!(self.input[self.position - 1], b'e' | b'E')
            {
                self.position += 1;
            } else {
                break;
            }
        }
        if start == self.position {
            return Err(math_parse_error("expected numeric value"));
        }
        std::str::from_utf8(&self.input[start..self.position])
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .ok_or_else(|| math_parse_error("invalid numeric value"))
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(u8::is_ascii_whitespace) {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.position).copied()
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn result_uses_canonical_sandbox() {
        let output = MathTool
            .run(serde_json::json!({"expression": "2 + 2"}))
            .await
            .unwrap();
        assert!(output.starts_with("<external_data source=\"math\" trust=\"untrusted\">"));
        assert_eq!(output.matches("</external_data>").count(), 1);
    }

    #[test]
    fn rejects_non_whitelisted_identifier() {
        assert!(validate_expression("system(1)").is_err());
    }
}
