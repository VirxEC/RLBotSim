use crate::flat::GameMode;
use std::{error::Error, fmt::Display, str::FromStr};

#[derive(Clone, Debug)]
pub struct EnumFromStrError {
    name: String,
}

impl Display for EnumFromStrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid value for enum - {}", self.name)
    }
}

impl Error for EnumFromStrError {}

impl FromStr for GameMode {
    type Err = EnumFromStrError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "soccer" | "soccar" => Ok(Self::Soccer),
            "hoops" => Ok(Self::Hoops),
            "dropshot" => Ok(Self::Dropshot),
            "rumble" => Ok(Self::Rumble),
            "heatseeker" => Ok(Self::Heatseeker),
            "hockey" | "snowday" => Ok(Self::Hockey),
            _ => Err(Self::Err { name: s.to_string() }),
        }
    }
}
