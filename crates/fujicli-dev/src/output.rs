use std::{
    fs::File,
    io::{self, Write as _},
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context as _, bail};
use tempfile::Builder as TempFileBuilder;

#[derive(Clone, Debug)]
pub struct NewOutput(PathBuf);

impl FromStr for NewOutput {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "-" {
            bail!("discovery artifacts require a new file path; stdout is not allowed");
        }
        Ok(Self(PathBuf::from(value)))
    }
}

impl NewOutput {
    pub fn write_all(&self, data: &[u8]) -> anyhow::Result<()> {
        let directory = self
            .0
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut file = TempFileBuilder::new()
            .prefix(".fujicli-dev-")
            .suffix(".tmp")
            .tempfile_in(directory)
            .context("creating private backup output transaction")?;
        file.write_all(data)?;
        file.flush()?;
        file.as_file().sync_all()?;
        drop(
            file.persist_noclobber(&self.0)
                .context("committing backup output without overwrite")?,
        );
        sync_directory(directory)?;
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::NewOutput;

    #[test]
    fn private_discovery_payload_cannot_be_sent_to_stdout() {
        let error = NewOutput::from_str("-").expect_err("stdout must be rejected");

        assert!(error.to_string().contains("stdout is not allowed"));
    }
}
