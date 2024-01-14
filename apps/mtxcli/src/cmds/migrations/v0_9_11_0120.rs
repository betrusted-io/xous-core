use std::io::Error;

use crate::cmds::migrations::MigrationApi;
use crate::cmds::{CommonEnv, FILTER_KEY};
use crate::migration_api;

#[derive(Debug)]
pub struct V0_9_11_0120 {}
impl V0_9_11_0120 {
    pub fn new() -> Self { V0_9_11_0120 {} }
}

impl<'a> MigrationApi<'a> for V0_9_11_0120 {
    migration_api!("v0.9.11-0120");

    fn process(&self, common: &mut CommonEnv) -> Result<bool, Error> {
        log::info!("Running migration for: {}", self.version());
        // we need to provoke the creation of a new/updated filter
        common.unset(FILTER_KEY)?;
        Ok(true)
    }
}
