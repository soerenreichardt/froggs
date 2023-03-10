use std::{rc::Rc, cell::RefCell};

use crate::{context::Context, expression::{Expression, UnaryOperator, BinaryOperator, MaterializableExpression, LogicalOperator}, error::Error, token::Lexeme, statement::Statement, environment::{Environment}, Materializable, callable::{Callable, DeclaredFunction, Clock}, value::FroxValue};
use crate::error::Result;

pub struct Interpreter<'a> {
    context: Context,
    environment: Rc<RefCell<Environment<'a>>>,
    pub(crate) globals: Rc<RefCell<Environment<'a>>>
}

impl<'a> Interpreter<'a> {
    pub(crate) fn new(source: Rc<str>, environment: Rc<RefCell<Environment<'a>>>) -> Self {
        environment.borrow_mut().define("clock".to_string(), FroxValue::Clock(Clock {}));
        Interpreter { context: Context::new(source), environment: environment.clone(), globals: environment.clone() }
    }

    pub(crate) fn interpret<F: FnMut(String) -> ()>(&mut self, statements: &Vec<Statement>, print_stream: &mut F) -> Result<()> {
        for statement in statements {
            match self.execute(statement, print_stream) {
                Err(error) => self.context.collect_error(error),
                _ => ()
            };
        }

        self.context.flush_errors(())
    } 

    fn execute<F: FnMut(String) -> ()>(&mut self, statement: &Statement, print_stream: &mut F) -> Result<()> {
        match statement {
            Statement::Expression(expression) => self.evaluate(&expression, print_stream).map(|_| ()),
            Statement::Print(expression) => {
                let value = self.evaluate(expression, print_stream)?;
                print_stream(value.to_string());
                Ok(())
            },
            Statement::Var(lexeme, initializer) => {
                let initial_value = match initializer {
                    Some(expression) => self.evaluate(expression, print_stream),
                    None => Ok(FroxValue::Nil)
                }?;

                self.environment.borrow_mut().define(lexeme.materialize(&self.context).to_string(), initial_value);
                Ok(())
            }
            Statement::Block(statements) => self.execute_block(statements, Environment::new_inner(self.environment.clone()).into(), print_stream),
            Statement::If(condition, then_branch, else_branch) => self.execute_condition(condition, then_branch, else_branch, print_stream),
            Statement::While(condition, body) => self.execute_while_loop(condition, body, print_stream),
            Statement::Function(name, parameters, body) => self.execute_function_declaration(name, parameters, body, print_stream),
            Statement::Return(value) => self.execute_return(value, print_stream)
        }
    }

    pub(crate) fn execute_block<F: FnMut(String) -> ()>(&mut self, statements: &Vec<Statement>, nested_ennvironment: Rc<RefCell<Environment<'a>>>, print_stream: &mut F) -> Result<()> {
        let previous_environment = std::mem::replace(&mut self.environment, nested_ennvironment);
        let mut execute_statements = || -> Result<()> {
            for statement in statements {
                self.execute(statement, print_stream)?;
            }
            return Ok(())
        };
        let execution_result = execute_statements();
        self.environment = previous_environment;
        execution_result
    }

    fn execute_condition<F: FnMut(String) -> ()>(&mut self, condition: &MaterializableExpression, then_branch: &Box<Statement>, else_branch: &Option<Box<Statement>>, print_stream: &mut F) -> Result<()> {
        if self.evaluate(condition, print_stream)?.to_boolean()? {
            return self.execute(&then_branch, print_stream);
        } 
        match else_branch {
            Some(statement) => self.execute(&statement, print_stream),
            None => {
                Ok(())
            }
        }
    }

    fn execute_while_loop<F: FnMut(String) -> ()>(&mut self, condition: &MaterializableExpression, body: &Box<Statement>, print_stream: &mut F) -> Result<()> {
        while self.evaluate(condition, print_stream)?.to_boolean()? {
            self.execute(body, print_stream)?;
        }
        Ok(())
    }

    fn execute_function_declaration<F: FnMut(String) -> ()>(&mut self, name: &Lexeme, parameters: &[Lexeme], body: &[Statement], print_stream: F) -> Result<()> {
        let name: Rc<str> = name.materialize(&self.context).into();
        let parameters: Vec<Rc<str>> = parameters.iter().map(|lexeme| lexeme.materialize(&self.context).into()).collect::<Vec<_>>();
        let body: Rc<Vec<Statement>> = Rc::new(body.to_vec());

        let declared_function = DeclaredFunction { name: name.clone(), parameters, body };
        self.environment.borrow_mut().define(name.to_string(), FroxValue::Function(declared_function));
        Ok(())
    }

    fn execute_return<F: FnMut(String) -> ()>(&mut self, value: &Option<MaterializableExpression>, print_stream: &mut F) -> Result<()> {
        let evaluated_value = match value {
            Some(value) => self.evaluate(value, print_stream)?,
            _ => FroxValue::Nil
        };
        Err(Error::ReturnCall(evaluated_value))
    }

    pub(crate) fn evaluate<F: FnMut(String) -> ()>(&mut self, MaterializableExpression { expression, lexeme }: &MaterializableExpression, print_stream: &mut F) -> Result<FroxValue> {
        match expression {
            Expression::Grouping(inner_expression) => self.evaluate(inner_expression.as_ref(), print_stream),
            Expression::Unary(operator, inner_expression) => {
                let right = self.evaluate(&inner_expression, print_stream)?;
                match operator {
                    UnaryOperator::Minus => -right,
                    UnaryOperator::Not => !right
                }
            },
            Expression::Binary(left, right, operator) => {
                let lhs = self.evaluate(&left, print_stream)?;
                let rhs = self.evaluate(&right, print_stream)?;
                match operator {
                    BinaryOperator::Subtract => lhs - rhs,
                    BinaryOperator::Divide => lhs / rhs,
                    BinaryOperator::Multiply => lhs * rhs,
                    BinaryOperator::Add => lhs + rhs,

                    BinaryOperator::GreaterThan => lhs.greater_than(rhs),
                    BinaryOperator::GreaterThenOrEqual => lhs.greater_than_or_equal(rhs),
                    BinaryOperator::LessThan => lhs.less_than(rhs),
                    BinaryOperator::LessThanOrEqual => lhs.less_than_or_equal(rhs),
                    BinaryOperator::Compare => lhs.equals(rhs),
                    BinaryOperator::CompareNot => lhs.not_equals(rhs)
                }
            },
            Expression::Literal(literal_value) => Ok(FroxValue::from_literal_value(literal_value)),
            Expression::Variable(lexeme) => self.environment.borrow().get(lexeme.materialize(&self.context).to_string(), lexeme),
            Expression::Assigment(lexeme, expression) => self.assignment(expression, lexeme, print_stream),
            Expression::Logical(left, right, operator) => self.logical(left, right, operator, print_stream),
            Expression::Call(callee, _, arguments) => self.call(callee, arguments, print_stream),
        }.map_err(|error| Error::FroxError(Error::format_interpreter_error(&error, lexeme, &self.context.source)))
    }

    fn assignment<F: FnMut(String) -> ()>(&mut self, expression: &MaterializableExpression, lexeme: &Lexeme, print_stream: &mut F) -> Result<FroxValue> {
        let value = self.evaluate(&expression, print_stream)?;
        self.environment.borrow_mut().assign(lexeme.materialize(&self.context).to_string(), value, lexeme)
    }

    fn logical<F: FnMut(String) -> ()>(&mut self, lhs: &MaterializableExpression, rhs: &MaterializableExpression, operator: &LogicalOperator, print_stream: &mut F) -> Result<FroxValue> {
        let left = self.evaluate(lhs, print_stream)?;

        let boolean_value = left.clone().to_boolean()?;
        let return_left = match operator {
            LogicalOperator::Or => boolean_value,
            LogicalOperator::And => !boolean_value
        };

        if return_left {
            return Ok(left.clone());
        }
        self.evaluate(rhs, print_stream)
    }

    fn call<F: FnMut(String) -> ()>(&mut self, callee: &MaterializableExpression, arguments: &Vec<Box<MaterializableExpression>>, print_stream: &mut F) -> Result<FroxValue> {
        let evaluated_callee = self.evaluate(callee, print_stream)?;

        let mut evaluated_arguments = Vec::new();
        for argument in arguments {
            evaluated_arguments.push(self.evaluate(argument, print_stream)?);
        }

        match evaluated_callee {
            FroxValue::Function(callable) => {
                if callable.arity() != evaluated_arguments.len() as u8 {
                    return Err(Error::InterpreterError(format!("Expected {} arguments but got {}", callable.arity(), evaluated_arguments.len())))
                }
                callable.call(evaluated_arguments, self, print_stream)
            },
            _ => Err(Error::InterpreterError("Invalid invocation target".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::expression::LiteralValue;

    use super::*;

    #[test]
    fn should_evaluate_arithmetic_expressions() {
        let actual = evaluate_binary_arithmetic(1.0, 2.0, BinaryOperator::Add);
        assert_eq!(3.0, actual);

        let actual = evaluate_binary_arithmetic(1.0, 2.0, BinaryOperator::Subtract);
        assert_eq!(-1.0, actual);

        let actual = evaluate_binary_arithmetic(7.0, 2.0, BinaryOperator::Divide);
        assert_eq!(3.5, actual);

        let actual = evaluate_binary_arithmetic(3.0, 2.0, BinaryOperator::Multiply);
        assert_eq!(6.0, actual);
    }

    #[test]
    fn should_evaluate_comparisons() {
        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(42.0), BinaryOperator::Compare);
        assert_eq!(true, actual, "42.0 == 42.0");

        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(42.0), BinaryOperator::CompareNot);
        assert_eq!(false, actual, "42.0 != 42.0");

        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(43.0), BinaryOperator::Compare);
        assert_eq!(false, actual, "42.0 == 43.0");

        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(43.0), BinaryOperator::CompareNot);
        assert_eq!(true, actual, "42.0 != 43.0");

        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(42.0), BinaryOperator::LessThan);
        assert_eq!(false, actual, "42.0 < 42.0");

        let actual = evaluate_comparison(LiteralValue::Number(42.0), LiteralValue::Number(42.0), BinaryOperator::LessThanOrEqual);
        assert_eq!(true, actual, "42.0 <= 42.0");

        let actual = evaluate_comparison(LiteralValue::Nil, LiteralValue::Nil, BinaryOperator::Compare);
        assert_eq!(true, actual, "Nil == Nil");

        let actual = evaluate_comparison(LiteralValue::Nil, LiteralValue::String("foo".into()), BinaryOperator::Compare);
        assert_eq!(false, actual, "Nil == \"foo\"");
    }

    #[test]
    fn should_concatenate_strings() {
        let expression = Expression::Binary(
            Box::new(Expression::Literal(LiteralValue::String("foo".into())).wrap_default()), 
            Box::new(Expression::Literal(LiteralValue::String("bar".into())).wrap_default()),
            BinaryOperator::Add
        ).wrap_default();
        let environment = Environment::new();
        let mut interpreter = Interpreter::new("".into(), environment.into());
        let value = match interpreter.evaluate(&expression, &mut |_| ()) {
            Ok(FroxValue::String(value)) => value,
            _ => panic!("{:?}", expression)
        };

        assert_eq!("foobar", value, "foo + bar")
    }

    fn evaluate_binary_arithmetic(lhs: f64, rhs: f64, operator: BinaryOperator) -> f64 {
        let expression = Expression::Binary(
            Box::new(Expression::Literal(LiteralValue::Number(lhs)).wrap_default()), 
            Box::new(Expression::Literal(LiteralValue::Number(rhs)).wrap_default()), 
            operator
        ).wrap_default();
        let environment = Environment::new();
        let mut interpreter = Interpreter::new("".into(), environment.into());
        match interpreter.evaluate(&expression, &mut |_| ()) {
            Ok(FroxValue::Number(value)) => value,
            _ => panic!("{:?}", expression)
        }
    }

    fn evaluate_comparison(lhs: LiteralValue, rhs: LiteralValue, operator: BinaryOperator) -> bool {
        let expression = Expression::Binary(
            Box::new(Expression::Literal(lhs).wrap_default()), 
            Box::new(Expression::Literal(rhs).wrap_default()), 
            operator
        ).wrap_default();
        let environment = Environment::new();
        let mut interpreter = Interpreter::new("".into(), environment.into());
        match interpreter.evaluate(&expression, &mut |_| ()) {
            Ok(FroxValue::Boolean(value)) => value,
            _ => panic!("{:?}", expression)
        }
    }
}