use rkyv::{Archive, Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[allow(dead_code)]
#[derive(Archive, Serialize, Deserialize, Debug)]
pub enum LinkState {
    Enabled,
    EnabledWithApproval,
    Disabled,
}

impl fmt::Display for LinkState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for LinkState {
    type Err = ();

    fn from_str(input: &str) -> Result<LinkState, Self::Err> {
        match input {
            "Enabled" => Ok(LinkState::Enabled),
            "EnabledWithApproval" => Ok(LinkState::EnabledWithApproval),
            "Disabled" => Ok(LinkState::Disabled),
            _ => Err(()),
        }
    }
}
