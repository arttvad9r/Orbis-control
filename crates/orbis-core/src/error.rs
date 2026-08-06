//! Ошибки доменной модели.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Ошибки, возникающие в платформенно-независимой модели.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoreError {
    /// Значение вне допустимого диапазона.
    OutOfRange {
        /// имя величины
        field: String,
        /// значение
        value: String,
        /// минимум
        min: String,
        /// максимум
        max: String,
    },
    /// Инвариант нарушен (например, температуры в кривой не монотонны).
    Invariant {
        /// поле
        field: String,
        /// описание нарушения
        message: String,
    },
    /// Некорректное строковое представление enum.
    Parse {
        /// тип
        r#type: String,
        /// исходная строка
        input: String,
    },
}

impl CoreError {
    /// Значение вне диапазона.
    pub fn out_of_range(field: &str, value: impl fmt::Display, min: impl fmt::Display, max: impl fmt::Display) -> Self {
        Self::OutOfRange {
            field: field.to_string(),
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        }
    }

    /// Нарушение инварианта.
    pub fn invariant(field: &str, message: impl fmt::Display) -> Self {
        Self::Invariant {
            field: field.to_string(),
            message: message.to_string(),
        }
    }

    /// Ошибка разбора строки.
    pub fn parse(r#type: &str, input: impl fmt::Display) -> Self {
        Self::Parse {
            r#type: r#type.to_string(),
            input: input.to_string(),
        }
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { field, value, min, max } => {
                write!(f, "{field}: значение {value} вне диапазона [{min}, {max}]")
            }
            Self::Invariant { field, message } => write!(f, "{field}: {message}"),
            Self::Parse { r#type, input } => write!(f, "не удалось разобрать {type} из '{input}'"),
        }
    }
}

impl std::error::Error for CoreError {}
