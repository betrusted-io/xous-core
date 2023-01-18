//! Migrations
//!
//! Runs migrations up to the current version

use std::io::Error;
use crate::cmds::{CommonEnv,VERSION_KEY};
use locales::t;

mod v0_9_11_0120;  use v0_9_11_0120::*;

const DEFAULT_VERSION: &str = "0";

pub trait MigrationApi<'a> {
    // created with migration_api! macro
    // returns my version
    fn version(&self) -> &'static str;
    // checks if the migration should be applied
    fn applies(&self, version: &str) -> bool;
    // run the migration
    fn process(&self, common: &mut CommonEnv) -> Result<bool, Error>;
}

// the argument to this macro is the command verb
#[macro_export]
macro_rules! migration_api {
    ($version:expr) => { // NOTE the expr will have literal quotes
        fn version(&self) -> &'static str {
            $version
        }

        fn applies(&self, version: &str) -> bool {
            if version < self.version() {
                true
            } else {
                false
            }
        }
    };
}

/// Run migrations as needed
pub fn run_migrations(common: &mut CommonEnv) {
    let version = common.get_default(VERSION_KEY, DEFAULT_VERSION);
    if version.ne(&common.version) {
        let running_migrations = t!("mtxcli.running.migrations", xous::LANG);
        let msg = format!("{} {} .. {}",
                          running_migrations, version, common.version);
        log::info!("{}", msg);
        common.send_async_msg(&msg);
        let mut migrations: Vec<Box<dyn MigrationApi>> = Vec::new();
        migrations.push(Box::new(V0_9_11_0120::new()));
        for migration in migrations.iter() {
            if migration.applies(&version) {
                match migration.process(common) {
                    Ok(boolean) => {
                        let migration_completed = t!("mtxcli.migration.completed", xous::LANG);
                        let msg = format!("{}: {}: {}",
                                          migration_completed, migration.version(), boolean);
                        common.send_async_msg(&msg);
                        if boolean {
                            common.set(VERSION_KEY, migration.version())
                                .expect("cannot set _version");
                        }
                    },
                    Err(e) => {
                        log::error!("error running migration: {}: {:?}",
                                    migration.version(), e);
                    }
                }
            }
        }
        common.set(VERSION_KEY, &common.version.clone())
            .expect("cannot set _version");
    }
}
