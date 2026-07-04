// Operator types for nex-calc.
//
// Each operator is a directional function: it maps two operand Facts
// to a new result Fact at a new coordinate in the FIH state space.

use std::fmt;

/// Arithmetic operators that define transitions in the FIH space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Add,
    Sub,
    Mul,
    Div,
}

impl OpType {
    /// Apply this operator to two number Facts.
    ///
    /// The computation itself is trivial; the significance lies in the
    /// fact that this is a coordinate-space traversal, not a CPU instruction.
    pub fn apply(&self, lhs: i64, rhs: i64) -> Result<i64, CalcOpError> {
        match self {
            OpType::Add => Ok(lhs + rhs),
            OpType::Sub => Ok(lhs - rhs),
            OpType::Mul => Ok(lhs * rhs),
            OpType::Div => {
                if rhs == 0 {
                    Err(CalcOpError::DivisionByZero)
                } else {
                    Ok(lhs / rhs)
                }
            }
        }
    }

    /// Symbol used in display.
    pub fn symbol(&self) -> &str {
        match self {
            OpType::Add => "+",
            OpType::Sub => "-",
            OpType::Mul => "*",
            OpType::Div => "/",
        }
    }

    /// Parse from a command string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add" | "+" => Some(OpType::Add),
            "sub" | "-" => Some(OpType::Sub),
            "mul" | "*" => Some(OpType::Mul),
            "div" | "/" => Some(OpType::Div),
            _ => None,
        }
    }
}

impl fmt::Display for OpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpType::Add => write!(f, "add"),
            OpType::Sub => write!(f, "sub"),
            OpType::Mul => write!(f, "mul"),
            OpType::Div => write!(f, "div"),
        }
    }
}

/// Errors that can occur during operator application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcOpError {
    DivisionByZero,
}

impl fmt::Display for CalcOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalcOpError::DivisionByZero => write!(f, "division by zero"),
        }
    }
}
