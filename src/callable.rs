use std::{time::UNIX_EPOCH, rc::Rc};

use crate::{interpreter::{FroxValue, Interpreter}, statement::Statement, environment::Environment};
use crate::error::Result;

pub(crate) trait Callable {
    fn name(&self) -> Rc<str>;

    fn arity(&self) -> u8;

    fn call<F: FnMut(String) -> ()>(&self, arguments: Vec<FroxValue>, interpreter: &mut Interpreter, print_stream: &mut F) -> Result<FroxValue>;
}

#[derive(PartialEq, Clone)]
pub struct DeclaredFunction {
    name: Rc<str>, 
    parameters: Vec<Rc<str>>,
    body: Rc<Vec<Statement>>
}

impl Callable for DeclaredFunction {
    fn name(&self) -> Rc<str> {
        self.name.clone()
    }

    fn arity(&self) -> u8 {
        self.parameters.len() as u8
    }

    fn call<F: FnMut(String) -> ()>(&self, arguments: Vec<FroxValue>, interpreter: &mut Interpreter, print_stream: &mut F) -> Result<FroxValue> {
        let mut environment = Environment::new_inner(interpreter.globals.clone());
        for (parameter, argument) in self.parameters.iter().zip(arguments.iter()) {
            environment.define(parameter.to_string(), argument.clone());
        }

        interpreter.execute_block(&self.body, environment.into(), print_stream)?;
        Ok(FroxValue::Nil)
    }
}

#[derive(PartialEq, Clone)]
pub struct Clock;

impl Callable for Clock {
    fn name(&self) -> Rc<str> {
        "clock".into()
    }

    fn arity(&self) -> u8 {
        0
    }

    fn call<F: FnMut(String) -> ()>(&self, arguments: Vec<FroxValue>, interpreter: &mut Interpreter, print_stream: &mut F) -> Result<FroxValue> {
        let now = std::time::SystemTime::now();
        let epoch_millis = now.duration_since(UNIX_EPOCH).expect("").as_millis() as f64;
        Ok(FroxValue::Number(epoch_millis))
    }
}