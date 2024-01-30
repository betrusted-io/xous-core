use serde::{Deserialize, Serialize};
use serde_json;
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Serialize_repr, Debug, Deserialize_repr, PartialEq)]
#[repr(u8)]
pub enum EntryType {
    Login = 1,
    SecureNote = 2,
    Card = 3,
    Identity = 4,
}

#[derive(Serialize, Debug, Deserialize)]
pub struct Entry {
    #[serde(rename = "type")]
    pub entry_type: EntryType,
    pub name: String,
    pub login: Option<Login>,
    pub notes: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Login {
    pub username: Option<String>,
    pub password: Option<String>,
    pub totp: Option<String>,
}

#[derive(Debug)]
pub enum LoginSanificationError {
    BadUsername,
    BadPassword,
}

impl std::fmt::Display for LoginSanificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadUsername => write!(f, "bad username format"),
            Self::BadPassword => write!(f, "bad password format"),
        }
    }
}

impl std::error::Error for LoginSanificationError {}

impl Login {
    pub fn sane(&self) -> Result<(), LoginSanificationError> {
        if self.username.is_none() {
            return Err(LoginSanificationError::BadUsername);
        } else {
            if self.username.as_ref().unwrap_or(&String::new()).is_empty() {
                return Err(LoginSanificationError::BadUsername);
            }
        }

        if self.password.is_none() {
            return Err(LoginSanificationError::BadPassword);
        } else {
            if self.password.as_ref().unwrap_or(&String::new()).is_empty() {
                return Err(LoginSanificationError::BadPassword);
            }
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Items {
    pub items: Vec<Entry>,
}

impl Items {
    pub fn logins(&self) -> Vec<&Entry> {
        let ret: Vec<&Entry> = self
            .items
            .iter()
            .filter(|element| element.entry_type == EntryType::Login && element.login.is_some())
            .collect();
        ret
    }
}

#[derive(Debug)]
pub enum FormatError {
    SerdeError(serde_json::Error),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SerdeError(err) => write!(f, "bitwarden deserialization error, {:?}", err),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<serde_json::Error> for FormatError {
    fn from(e: serde_json::Error) -> FormatError { FormatError::SerdeError(e) }
}

impl TryFrom<Vec<u8>> for Items {
    type Error = FormatError;

    fn try_from(data: Vec<u8>) -> Result<Items, Self::Error> {
        let ret: Items = serde_json::from_slice(&data)?;
        Ok(ret)
    }
}

impl TryFrom<std::fs::File> for Items {
    type Error = FormatError;

    fn try_from(data: std::fs::File) -> Result<Items, Self::Error> {
        let ret: Items = serde_json::from_reader(data)?;
        Ok(ret)
    }
}
