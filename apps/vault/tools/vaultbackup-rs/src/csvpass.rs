use csv;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Deserialize)]
pub struct Entry {
    pub site: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Items {
    pub items: Vec<Entry>,
}

impl Items {
    pub fn logins(&self) -> Vec<&Entry> {
        let ret: Vec<&Entry> = self.items.iter().filter(|element| element.site.is_some()).collect();
        ret
    }
}

impl std::error::Error for CsvError {}

#[derive(Debug)]
pub enum CsvError {
    Err(String),
}
impl From<csv::Error> for CsvError {
    fn from(err: csv::Error) -> Self { CsvError::Err(format!("{:?}", err)) }
}

impl std::fmt::Display for CsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Err(err) => write!(f, "CSV deserialization error, {:?}", err),
        }
    }
}

impl TryFrom<std::fs::File> for Items {
    type Error = CsvError;

    fn try_from(data: std::fs::File) -> Result<Items, Self::Error> {
        let mut ret = Vec::<Entry>::new();
        let mut rdr = csv::Reader::from_reader(data);
        let mut iter = rdr.deserialize();

        loop {
            if let Some(result) = iter.next() {
                let record: Entry = result?;
                ret.push(record);
            } else {
                break;
            }
        }
        Ok(Items { items: ret })
    }
}
