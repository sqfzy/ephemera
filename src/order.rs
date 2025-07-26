#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl TryFrom<&str> for Side {
    type Error = eyre::Report;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "buy" => Ok(Side::Buy),
            "sell" => Ok(Side::Sell),
            _ => eyre::bail!("Invalid order side: '{}'", value),
        }
    }
}
