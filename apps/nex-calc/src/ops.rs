// Operator types for nex-calc.
//
// Each operator is a directional function: it maps operand Facts
// to a new result Fact at a new coordinate in the FIH state space.
// The enum contains arithmetic, bitwise, and placeholder operators
// for vector/transform primitives — demonstrating that any computation
// fits the FIH traversal model.

use std::fmt;

/// Arithmetic and computational operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    // ── Arithmetic (binary: (lhs, rhs) → result) ────────────────
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Min,
    Max,

    // ── Arithmetic (unary: (lhs, _) → result, rhs ignored) ──────
    Neg,
    Abs,
    Sqrt,
    Fac,

    // ── Bitwise (binary) ─────────────────────────────────────────
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    // ── Bitwise (unary) ──────────────────────────────────────────
    BitNot,

    // ── Vector/transform primitives (scalar placeholder) ─────────
    /// Dot product of two vectors. Scalar form returns an error.
    MatMul,
    /// Fast Fourier Transform. Scalar form returns an error.
    FFT,
    /// Convolution. Scalar form returns an error.
    Conv,
}

impl OpType {
    /// How many operand Facts this operator reads from the FIH space.
    pub fn arity(&self) -> usize {
        match self {
            OpType::Neg | OpType::Abs | OpType::Sqrt | OpType::Fac | OpType::BitNot => 1,
            _ => 2,
        }
    }

    /// Apply this operator to operand values.
    ///
    /// For binary ops, `lhs` and `rhs` are both used.
    /// For unary ops, only `lhs` is used; `rhs` is ignored.
    /// Vector/placeholder ops return `VectorRequired` with guidance.
    pub fn apply(&self, lhs: i64, rhs: i64) -> Result<i64, CalcOpError> {
        match self {
            // Binary arithmetic
            OpType::Add => lhs.checked_add(rhs).ok_or(CalcOpError::Overflow),
            OpType::Sub => lhs.checked_sub(rhs).ok_or(CalcOpError::Overflow),
            OpType::Mul => lhs.checked_mul(rhs).ok_or(CalcOpError::Overflow),
            OpType::Div => {
                if rhs == 0 {
                    Err(CalcOpError::DivisionByZero)
                } else {
                    lhs.checked_div(rhs).ok_or(CalcOpError::Overflow)
                }
            }
            OpType::Rem => {
                if rhs == 0 {
                    Err(CalcOpError::DivisionByZero)
                } else {
                    Ok(lhs % rhs)
                }
            }
            OpType::Pow => {
                if rhs < 0 {
                    return Err(CalcOpError::DomainError(
                        "negative exponent not supported for integers".into(),
                    ));
                }
                lhs.checked_pow(rhs as u32).ok_or(CalcOpError::Overflow)
            }
            OpType::Min => Ok(lhs.min(rhs)),
            OpType::Max => Ok(lhs.max(rhs)),

            // Unary arithmetic (rhs ignored)
            OpType::Neg => lhs.checked_neg().ok_or(CalcOpError::Overflow),
            OpType::Abs => Ok(lhs.abs()),
            OpType::Sqrt => match lhs {
                n if n < 0 => Err(CalcOpError::DomainError("sqrt of negative".into())),
                n => Ok((n as f64).sqrt() as i64),
            },
            OpType::Fac => {
                if lhs < 0 {
                    return Err(CalcOpError::DomainError("factorial of negative".into()));
                }
                let mut result: i64 = 1;
                for i in 2..=lhs {
                    result = result.checked_mul(i).ok_or(CalcOpError::Overflow)?;
                }
                Ok(result)
            }

            // Binary bitwise
            OpType::BitAnd => Ok(lhs & rhs),
            OpType::BitOr => Ok(lhs | rhs),
            OpType::BitXor => Ok(lhs ^ rhs),
            OpType::Shl => {
                if rhs < 0 {
                    return Err(CalcOpError::DomainError("negative shift".into()));
                }
                lhs.checked_shl(rhs as u32).ok_or(CalcOpError::Overflow)
            }
            OpType::Shr => {
                if rhs < 0 {
                    return Err(CalcOpError::DomainError("negative shift".into()));
                }
                Ok(lhs.wrapping_shr(rhs as u32))
            }

            // Unary bitwise
            OpType::BitNot => Ok(!lhs),

            // Vector/transform placeholders
            OpType::MatMul => Err(CalcOpError::VectorRequired(
                "MatMul requires vector/matrix operands".into(),
            )),
            OpType::FFT => Err(CalcOpError::VectorRequired(
                "FFT requires a vector operand".into(),
            )),
            OpType::Conv => Err(CalcOpError::VectorRequired(
                "Conv requires vector operands".into(),
            )),
        }
    }

    /// Short symbol for display.
    pub fn symbol(&self) -> &str {
        match self {
            OpType::Add => "+",
            OpType::Sub => "-",
            OpType::Mul => "*",
            OpType::Div => "/",
            OpType::Rem => "%",
            OpType::Pow => "^",
            OpType::Min => "min",
            OpType::Max => "max",
            OpType::Neg => "neg",
            OpType::Abs => "abs",
            OpType::Sqrt => "sqrt",
            OpType::Fac => "!",
            OpType::BitAnd => "&",
            OpType::BitOr => "|",
            OpType::BitXor => "^",
            OpType::Shl => "<<",
            OpType::Shr => ">>",
            OpType::BitNot => "~",
            OpType::MatMul => "@",
            OpType::FFT => "fft",
            OpType::Conv => "conv",
        }
    }

    /// Parse from a command string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add" | "+" => Some(OpType::Add),
            "sub" | "-" => Some(OpType::Sub),
            "mul" | "*" => Some(OpType::Mul),
            "div" | "/" => Some(OpType::Div),
            "rem" | "%" => Some(OpType::Rem),
            "pow" | "^" => Some(OpType::Pow),
            "min" => Some(OpType::Min),
            "max" => Some(OpType::Max),
            "neg" => Some(OpType::Neg),
            "abs" => Some(OpType::Abs),
            "sqrt" => Some(OpType::Sqrt),
            "fac" | "factorial" => Some(OpType::Fac),
            "and" | "&" => Some(OpType::BitAnd),
            "or" | "|" => Some(OpType::BitOr),
            "xor" => Some(OpType::BitXor),
            "shl" | "<<" => Some(OpType::Shl),
            "shr" | ">>" => Some(OpType::Shr),
            "bnot" | "~" => Some(OpType::BitNot),
            "matmul" | "@" => Some(OpType::MatMul),
            "fft" => Some(OpType::FFT),
            "conv" => Some(OpType::Conv),
            _ => None,
        }
    }
}

impl fmt::Display for OpType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.symbol())
    }
}

/// Errors that can occur during operator application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalcOpError {
    DivisionByZero,
    Overflow,
    DomainError(String),
    VectorRequired(String),
}

impl fmt::Display for CalcOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalcOpError::DivisionByZero => write!(f, "division by zero"),
            CalcOpError::Overflow => write!(f, "integer overflow"),
            CalcOpError::DomainError(msg) => write!(f, "domain error: {msg}"),
            CalcOpError::VectorRequired(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(OpType::Add.apply(3, 5), Ok(8));
    }
    #[test]
    fn test_sub() {
        assert_eq!(OpType::Sub.apply(10, 3), Ok(7));
    }
    #[test]
    fn test_mul() {
        assert_eq!(OpType::Mul.apply(6, 7), Ok(42));
    }
    #[test]
    fn test_div() {
        assert_eq!(OpType::Div.apply(42, 6), Ok(7));
    }
    #[test]
    fn test_div_zero() {
        assert_eq!(OpType::Div.apply(1, 0), Err(CalcOpError::DivisionByZero));
    }
    #[test]
    fn test_rem() {
        assert_eq!(OpType::Rem.apply(10, 3), Ok(1));
    }
    #[test]
    fn test_pow() {
        assert_eq!(OpType::Pow.apply(2, 10), Ok(1024));
    }
    #[test]
    fn test_min() {
        assert_eq!(OpType::Min.apply(3, 7), Ok(3));
    }
    #[test]
    fn test_max() {
        assert_eq!(OpType::Max.apply(3, 7), Ok(7));
    }
    #[test]
    fn test_neg() {
        assert_eq!(OpType::Neg.apply(42, 0), Ok(-42));
    }
    #[test]
    fn test_abs() {
        assert_eq!(OpType::Abs.apply(-5, 0), Ok(5));
    }
    #[test]
    fn test_sqrt() {
        assert_eq!(OpType::Sqrt.apply(16, 0), Ok(4));
    }
    #[test]
    fn test_sqrt_neg() {
        assert!(OpType::Sqrt.apply(-1, 0).is_err());
    }
    #[test]
    fn test_fac() {
        assert_eq!(OpType::Fac.apply(5, 0), Ok(120));
    }
    #[test]
    fn test_fac_neg() {
        assert!(OpType::Fac.apply(-1, 0).is_err());
    }
    #[test]
    fn test_bitand() {
        assert_eq!(OpType::BitAnd.apply(6, 3), Ok(2));
    }
    #[test]
    fn test_bitor() {
        assert_eq!(OpType::BitOr.apply(6, 3), Ok(7));
    }
    #[test]
    fn test_bitxor() {
        assert_eq!(OpType::BitXor.apply(6, 3), Ok(5));
    }
    #[test]
    fn test_shl() {
        assert_eq!(OpType::Shl.apply(3, 2), Ok(12));
    }
    #[test]
    fn test_shr() {
        assert_eq!(OpType::Shr.apply(12, 2), Ok(3));
    }
    #[test]
    fn test_bitnot() {
        assert_eq!(OpType::BitNot.apply(0i64, 0), Ok(!0i64));
    }
    #[test]
    fn test_overflow_returns_error() {
        assert_eq!(OpType::Mul.apply(i64::MAX, 2), Err(CalcOpError::Overflow));
    }
    #[test]
    fn test_parse_all() {
        for s in [
            "add", "sub", "mul", "div", "rem", "pow", "min", "max", "neg", "abs", "sqrt", "fac",
            "and", "or", "xor", "shl", "shr", "bnot", "matmul", "fft", "conv",
        ] {
            assert!(OpType::parse(s).is_some(), "failed to parse {s}");
        }
    }
    #[test]
    fn test_arity() {
        assert_eq!(OpType::Add.arity(), 2);
        assert_eq!(OpType::Neg.arity(), 1);
        assert_eq!(OpType::FFT.arity(), 2);
    }
}
