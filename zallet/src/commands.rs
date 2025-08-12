//! Zallet Subcommands

use std::path::{Path, PathBuf};

use abscissa_core::{Configurable, FrameworkError, FrameworkErrorKind, Runnable, config::Override};
use home::home_dir;

use crate::{
    cli::{EntryPoint, ZalletCmd},
    config::ZalletConfig,
    error::{Error, ErrorKind},
    fl,
};

mod example_config;
mod generate_mnemonic;
mod import_mnemonic;
mod init_wallet_encryption;
mod migrate_zcash_conf;
mod start;

#[cfg(feature = "rpc-cli")]
pub(crate) mod rpc_cli;

/// Ensures only a single Zallet process is using the data directory.
pub(crate) fn lock_datadir(datadir: &Path) -> Result<fmutex::Guard<'static>, Error> {
    use std::fs;

    let lockfile_path = resolve_datadir_path(datadir, Path::new(".lock"));

    {
        // Ensure that the lockfile exists on disk.
        let _ = fs::File::create(&lockfile_path).map_err(|e| {
            ErrorKind::Init.context(fl!(
                "err-init-failed-to-create-lockfile",
                path = lockfile_path.display().to_string(),
                error = e.to_string(),
            ))
        })?;
    }

    let guard = fmutex::try_lock_exclusive_path(&lockfile_path)
        .map_err(|e| {
            ErrorKind::Init.context(fl!(
                "err-init-failed-to-read-lockfile",
                path = lockfile_path.display().to_string(),
                error = e.to_string(),
            ))
        })?
        .ok_or_else(|| {
            ErrorKind::Init.context(fl!(
                "err-init-zallet-already-running",
                datadir = datadir.display().to_string(),
            ))
        })?;

    Ok(guard)
}

/// Resolves the requested path relative to the Zallet data directory.
pub(crate) fn resolve_datadir_path(datadir: &Path, path: &Path) -> PathBuf {
    // TODO: Do we canonicalize here? Where do we enforce any requirements on the
    // config's relative paths?
    datadir.join(path)
}

impl EntryPoint {
    /// Returns the data directory to use for this Zallet command.
    fn datadir(&self) -> Result<PathBuf, FrameworkError> {
        // TODO: Decide whether to make either the default datadir, or every datadir,
        // chain-specific.
        if let Some(datadir) = &self.datadir {
            Ok(datadir.clone())
        } else {
            // The XDG Base Directory Specification is widely misread as saying that
            // `$XDG_DATA_HOME` should be used for storing mutable user-generated data.
            // The specification actually says that it is the userspace version of
            // `/usr/share` and is for user-specific versions of the latter's files. And
            // per the Filesystem Hierarchy Standard:
            //
            // > The `/usr/share` hierarchy is for all read-only architecture independent
            // > data files.
            //
            // This has led to inconsistent beliefs about which of `$XDG_CONFIG_HOME` and
            // `$XDG_DATA_HOME` should be backed up, and which is safe to delete at any
            // time. See https://bsky.app/profile/str4d.xyz/post/3lsjbnpsbh22i for more
            // details.
            //
            // Given the above, we eschew the XDG Base Directory Specification entirely,
            // and use `$HOME/.zallet` as the default datadir. The config file provides
            // sufficient flexibility for individual users to use XDG paths at their own
            // risk (and with knowledge of their OS environment's behaviour).
            home_dir()
                .ok_or_else(|| {
                    FrameworkErrorKind::ComponentError
                        .context(fl!("err-init-cannot-find-home-dir"))
                        .into()
                })
                .map(|base| base.join(".zallet"))
        }
    }
}

impl Runnable for EntryPoint {
    fn run(&self) {
        self.cmd.run()
    }
}

impl Configurable<ZalletConfig> for EntryPoint {
    fn process_config(&self, _config: ZalletConfig) -> Result<ZalletConfig, FrameworkError> {
        // Load configuration using config-rs
        let datadir = self.datadir()?;
        let config_path = ZalletConfig::resolve_config_path(&datadir, self.config.as_deref());

        // Convert config-rs error to FrameworkError at the Abscissa boundary
        let mut config = ZalletConfig::load(config_path.as_deref())
            .map_err(|e| FrameworkErrorKind::ConfigError.context(e))?;

        // Set datadir from CLI argument
        config.datadir = Some(datadir);

        // Apply command-specific overrides
        match &self.cmd {
            ZalletCmd::Start(cmd) => cmd.override_config(config),
            _ => Ok(config),
        }
    }
}
