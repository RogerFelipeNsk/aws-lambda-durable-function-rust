use thiserror::Error;

pub const TEST_NUMBER: &str = "1111111111111111";
pub const TEST_EXP: &str = "01/2040";
pub const TEST_CVC: &str = "111";
pub const FAIL_NUMBER: &str = "0000000000000000";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PaymentError {
    #[error("dados do cartão inválidos")]
    InvalidCard,
    #[error("cartão recusado")]
    Declined,
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub number: String,
    pub exp: String,
    pub cvc: String,
}

pub fn normalize(n: &str) -> String {
    n.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn last4(n: &str) -> String {
    let digits = normalize(n);
    if digits.len() < 4 {
        digits
    } else {
        digits[digits.len() - 4..].to_string()
    }
}

pub fn validate(input: &Input) -> Result<(), PaymentError> {
    let number = normalize(&input.number);
    let exp = input.exp.trim();
    let cvc = input.cvc.trim();
    if number == FAIL_NUMBER && exp == TEST_EXP && cvc == TEST_CVC {
        return Err(PaymentError::Declined);
    }
    if number == TEST_NUMBER && exp == TEST_EXP && cvc == TEST_CVC {
        return Ok(());
    }
    Err(PaymentError::InvalidCard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_test_card() {
        validate(&Input {
            number: "1111 1111 1111 1111".into(),
            exp: TEST_EXP.into(),
            cvc: TEST_CVC.into(),
        })
        .unwrap();
    }

    #[test]
    fn declines_zero_card() {
        let err = validate(&Input {
            number: FAIL_NUMBER.into(),
            exp: TEST_EXP.into(),
            cvc: TEST_CVC.into(),
        })
        .unwrap_err();
        assert_eq!(err, PaymentError::Declined);
    }

    #[test]
    fn rejects_unknown() {
        let err = validate(&Input {
            number: "4111111111111111".into(),
            exp: TEST_EXP.into(),
            cvc: TEST_CVC.into(),
        })
        .unwrap_err();
        assert_eq!(err, PaymentError::InvalidCard);
    }

    #[test]
    fn last4_strips_separators() {
        assert_eq!(last4("1111-1111-1111-1111"), "1111");
    }
}
