use std::fmt;
use std::str::FromStr;

// The server environment to use:
#[derive(Clone, Debug)]
pub enum ServiceEnvironment {
    Live,
    Staging,
}

impl fmt::Display for ServiceEnvironment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for ServiceEnvironment {
    type Err = ();

    fn from_str(input: &str) -> Result<ServiceEnvironment, Self::Err> {
        match input {
            "Live" => Ok(ServiceEnvironment::Live),
            "Staging" => Ok(ServiceEnvironment::Staging),
            _ => Err(()),
        }
    }
}
